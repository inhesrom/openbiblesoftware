//! The Note editor (#186): a text field for a Note's text with Note formatting, used by the
//! Notes Tool's cards and the popup Note editor.
//!
//! - **Toolbar** above the field: **B**old, *I*talic, Heading, bulleted list, numbered list,
//!   block quote and link. Each acts on the selection (or the lines it touches; Heading, the
//!   line or heading it starts on) and gives the keys back to the field; pressing one, or
//!   reaching one with Tab or Shift+Tab (Space or Enter presses it), does not end the editing
//!   pass. Bold and italic mark each
//!   selected line on its own (empty lines skipped), since a span cannot run over a blank line
//!   or a list item's marker; the link button is off while the selection spans lines. What is
//!   bold, italic, a heading, a list item or a quote is read from the Note's CommonMark parse
//!   ([`obs_model::parse_note_text`], the one Markdown reader), never by scanning for markers
//!   here: Bold/Italic take off the delimiters of the span the parse found, a heading is
//!   rewritten whole from its source and inline content as parsed (any ATX or setext form),
//!   and Enter continues the item the parse found. A nested item is written under its parent's
//!   text, as CommonMark nests it, and so are the item's own paragraphs below it, past gaps. Bold or italic over lines that the change cuts (into a mark
//!   on each line, or blocks of their own) is cut into one span per line first, so each line
//!   keeps its formatting. A code span, link, image or autolink is opaque, never cut or
//!   re-aligned: one the selection takes in whole is copied as typed; while the selection (or
//!   the cursor) is partly in one, every button is off ([`PARTLY_COVERS_SPAN`]), and while a
//!   change would cut one at a line break, that button is off ([`SPAN_ACROSS_LINES`]). Then the
//!   text stays as it is, byte for byte ([`format_blocked`]). A link target goes between `<`
//!   and `>`.
//! - **Safety net** around every change the toolbar or a shortcut makes: it acts only on
//!   simple blocks (paragraphs, headings, quotes of paragraphs, lists of paragraphs one level
//!   deep, [`obs_model::note_unformattable`]), and the change is made only when the Note,
//!   read again, shows the same text, formatted the same, but for the mark asked for
//!   ([`shows_as_intended`]). Otherwise the button is off ([`FORMAT_NOT_HERE`]) and the text
//!   stays as it is, byte for byte.
//! - **Shortcuts**: Ctrl+B and Ctrl+I (Cmd on macOS). Typing `- `, `1. ` or `> ` at the start
//!   of a line starts a list or quote; Enter in an item continues it (the next number), and
//!   Enter in an empty item ends it.
//! - **Undo**: each toolbar or shortcut change is its own step of the field's undo (Ctrl+Z,
//!   Ctrl+Shift+Z), like typing; the saves of an editing pass stay one Annotation edit.
//! - **Drawn as written**: the field shows the Markdown, with headings larger, bold text in
//!   the (bold) display face, italics slanted, markup dimmed and links and Bible references
//!   in the link colour, so what is typed is what is stored.

use std::ops::Range;
use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Color32, FontId, Key, Modifiers, RichText, Stroke, Ui};
use obs_model::{NoteLineKind, NoteMarkKind};

use crate::icons::{self, Icon};
use crate::theme;

/// A change the toolbar or a shortcut makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    Bold,
    Italic,
    /// Cycles the line (or heading) the selection starts on through `#`, `##`, `###` and
    /// back to text.
    Heading,
    Bullet,
    Numbered,
    Quote,
    Link,
}

impl Format {
    const TOOLBAR: [Format; 7] = [
        Format::Bold,
        Format::Italic,
        Format::Heading,
        Format::Bullet,
        Format::Numbered,
        Format::Quote,
        Format::Link,
    ];

    /// The button's text, or its icon and the name a screen reader reads for it.
    fn label(self) -> Result<RichText, (Icon, &'static str)> {
        match self {
            Format::Bold => Ok(RichText::new("B").strong()),
            Format::Italic => Ok(RichText::new("I").italics()),
            Format::Heading => Ok(RichText::new("H")),
            Format::Bullet => Err((Icon::BulletList, "Bulleted list")),
            Format::Numbered => Ok(RichText::new("1.")),
            Format::Quote => Ok(RichText::new("“”")),
            Format::Link => Err((Icon::Link, "Link")),
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Format::Bold => "Bold (Ctrl+B)",
            Format::Italic => "Italic (Ctrl+I)",
            Format::Heading => "Heading: click again for a smaller one, then back to text",
            Format::Bullet => "Bulleted list (or type “- ” at the start of a line)",
            Format::Numbered => "Numbered list (or type “1. ” at the start of a line)",
            Format::Quote => "Block quote (or type “> ” at the start of a line)",
            Format::Link => {
                "Link: type a Bible reference (Rom 5:8) or a web address between the < and >"
            }
        }
    }
}

/// What the editor did this frame.
pub(super) struct NoteEditorOutput {
    pub(super) response: egui::Response,
    /// The keys left the editor this frame: neither the field nor a toolbar button has them,
    /// and the pointer is not pressing the toolbar. Moving them from the field to the toolbar
    /// (Tab, Shift+Tab, a press) is not leaving: the editing pass goes on.
    pub(super) left: bool,
}

/// The toolbar and the text field (`id`), `rows` high.
pub(super) fn note_editor(
    ui: &mut Ui,
    id: egui::Id,
    text: &mut String,
    rows: usize,
) -> NoteEditorOutput {
    let mut action = None;
    // Whether a toolbar button has the keys, or had them a moment ago.
    let (mut toolbar_focus, mut toolbar_lost) = (false, false);
    // A button pressed with Enter or Space: that key is the press, not typing for the field.
    let mut pressed_with_keys = false;
    let sel = selection(ui.ctx(), id, text);
    // Why a button is off for this selection, if it is.
    let off = |f: Format| format_blocked(text, sel.clone(), f);
    let toolbar = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for f in Format::TOOLBAR {
                let (b, name) = match f.label() {
                    Ok(text) => (egui::Button::new(text), None),
                    Err((icon, name)) => {
                        let img = icons::image(ui.ctx(), icon, icons::BODY, theme::c().ink);
                        (egui::Button::new(img), Some(name))
                    }
                };
                let b = b.frame(false).min_size(egui::vec2(24.0, 20.0));
                let why = off(f);
                let mut response = ui.add_enabled(why.is_none(), b);
                if let Some(name) = name {
                    response = response.accessible_name(name);
                }
                let response = response
                    .on_hover_text(f.hint())
                    .on_disabled_hover_text(why.unwrap_or_default());
                if let Some(why) = why {
                    ui.ctx()
                        .accesskit_node_builder(response.id, |n| n.set_description(why));
                }
                toolbar_focus |= response.has_focus();
                toolbar_lost |= response.lost_focus();
                if response.clicked() {
                    action = Some(f);
                    pressed_with_keys |= response.has_focus();
                }
            }
        })
        .response;
    // A press or click there: the field gives up the keys on one or the other.
    let on_toolbar = ui.input(|i| {
        i.pointer
            .interact_pos()
            .is_some_and(|p| toolbar.rect.contains(p))
            && (i.pointer.any_down() || i.pointer.any_released())
    });
    if ui.memory(|m| m.has_focus(id)) {
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::COMMAND, Key::B) {
                action = Some(Format::Bold);
            } else if i.consume_key(Modifiers::COMMAND, Key::I) {
                action = Some(Format::Italic);
            }
        });
        continue_list_on_enter(ui, id, text);
    }
    if pressed_with_keys {
        ui.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Key { key, .. } => !matches!(key, Key::Enter | Key::Space),
                egui::Event::Text(t) => t != " ",
                _ => true,
            });
        });
    }
    if let Some(f) = action {
        edit_in_place(ui.ctx(), id, text, |t, sel| apply_format(t, sel, f));
        ui.memory_mut(|m| m.request_focus(id));
    }
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
        let mut job = (*highlighted(ui, id, buf.as_str())).clone();
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let response = ui
        .add(
            egui::TextEdit::multiline(text)
                .id(id)
                .hint_text("Write a note…")
                .desired_rows(rows)
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter),
        )
        .accessible_name("Note");
    if on_toolbar {
        response.request_focus();
    }
    let inside = on_toolbar || toolbar_focus || response.has_focus();
    NoteEditorOutput {
        left: (response.lost_focus() || toolbar_lost) && !inside,
        response,
    }
}

/// Why the link button is off while the selection spans lines.
const LINK_ONE_LINE: &str = "Link: select text on one line";

/// The field's selection (char indices), as [`edit_in_place`] reads it.
fn selection(ctx: &egui::Context, id: egui::Id, text: &str) -> Range<usize> {
    let end = text.chars().count();
    let range = egui::TextEdit::load_state(ctx, id)
        .and_then(|s| s.cursor.char_range())
        .unwrap_or_else(|| CCursorRange::one(CCursor::new(end)));
    let sel = range.as_sorted_char_range();
    sel.start.0.min(end)..sel.end.0.min(end)
}

/// Run `change` on the field's text and selection (char indices), as one undo step of the
/// field, and put the selection it returns.
fn edit_in_place(
    ctx: &egui::Context,
    id: egui::Id,
    text: &mut String,
    change: impl FnOnce(&str, Range<usize>) -> (String, Range<usize>),
) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let end = text.chars().count();
    let range = state
        .cursor
        .char_range()
        .unwrap_or_else(|| CCursorRange::one(CCursor::new(end)));
    let sel = range.as_sorted_char_range();
    let sel = sel.start.0.min(end)..sel.end.0.min(end);
    let (new, new_sel) = change(text, sel);
    if new == *text {
        return;
    }
    let mut undoer = state.undoer();
    undoer.add_undo(&(range, text.clone()));
    let after = CCursorRange::two(CCursor::new(new_sel.start), CCursor::new(new_sel.end));
    undoer.add_undo(&(after, new.clone()));
    state.set_undoer(undoer);
    state.cursor.set_char_range(Some(after));
    egui::TextEdit::store_state(ctx, id, state);
    *text = new;
}

/// Enter at the end of a list item or quote line continues it; in an empty one it ends it.
fn continue_list_on_enter(ui: &Ui, id: egui::Id, text: &mut String) {
    let state = egui::TextEdit::load_state(ui.ctx(), id);
    let Some(range) = state.and_then(|s| s.cursor.char_range()) else {
        return;
    };
    if !range.is_empty() {
        return;
    }
    let at = range.primary.index.0;
    let Some(next) = list_continuation(text, at) else {
        return;
    };
    if !ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
        return;
    }
    edit_in_place(ui.ctx(), id, text, |_, _| next);
}

/// Byte index of char index `c` in `s`.
fn byte_at(s: &str, c: usize) -> usize {
    s.char_indices().nth(c).map_or(s.len(), |(b, _)| b)
}

/// What Enter does at char `at` of `text`, if it is in a list item or quote line (as the
/// Note's CommonMark parse reads it) past its marker: the new text and cursor. The next line
/// gets the line's marker as typed, a numbered item's number one more. Behind the safety net
/// (module docs): not in a block that is not simple, and only when the Note, read again,
/// shows the same text, formatted the same, line breaks and white space aside (Enter puts
/// in a line break); otherwise Enter is the field's own.
pub(super) fn list_continuation(text: &str, at: usize) -> Option<(String, Range<usize>)> {
    let b = byte_at(text, at);
    let line = touched_lines(text, b, b).remove(0);
    if in_unformattable(&obs_model::note_unformattable(text), &line) {
        return None;
    }
    let (new, sel) = continued(text, at)?;
    (shown_chars(text) == shown_chars(&new)).then_some((new, sel))
}

/// What `text` shows, character by character with its marks, line breaks and white space
/// left out.
fn shown_chars(text: &str) -> Vec<(char, Vec<obs_model::NoteShownMark>)> {
    obs_model::note_shown(text)
        .into_iter()
        .filter_map(|r| match r {
            obs_model::NoteShown::Text { text, marks } => Some((text, marks)),
            obs_model::NoteShown::Break => None,
        })
        .flat_map(|(text, marks)| {
            text.chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| (c, marks.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// [`list_continuation`] before the safety net.
fn continued(text: &str, at: usize) -> Option<(String, Range<usize>)> {
    let b = byte_at(text, at);
    let lines = obs_model::parse_note_text(text);
    let line = lines
        .iter()
        .find(|l| l.source.start <= b && b <= l.source.end)?;
    let (start, end) = (line.source.start, line.source.end);
    let marker = &text[start..line.content];
    let next = match line.kind {
        NoteLineKind::Bullet { .. } => marker.to_string(),
        NoteLineKind::Numbered { number, .. } => {
            let digits = marker.find(|c: char| c.is_ascii_digit())?;
            let len = marker[digits..]
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(marker.len() - digits);
            let n = number.saturating_add(1);
            format!("{}{n}{}", &marker[..digits], &marker[digits + len..])
        }
        NoteLineKind::Quote => format!("{} ", marker.trim_end()),
        _ => return None,
    };
    if b < line.content {
        return None;
    }
    if text[line.content..end].trim().is_empty() {
        // An empty item: Enter ends the list, leaving an empty line.
        let new = format!("{}{}", &text[..start], &text[end..]);
        let c = text[..start].chars().count();
        return Some((new, c..c));
    }
    let new = format!("{}\n{next}{}", &text[..b], &text[b..]);
    let c = at + 1 + next.chars().count();
    Some((new, c..c))
}

/// `text` with `f` applied to the selection `sel` (char indices), and the selection after;
/// `text` as it is while `f` is off for `sel` ([`format_blocked`]).
pub(super) fn apply_format(text: &str, sel: Range<usize>, f: Format) -> (String, Range<usize>) {
    if format_blocked(text, sel.clone(), f).is_some() {
        return (text.to_string(), sel);
    }
    formatted(text, sel, f)
}

/// `text` with `f` made on `sel` (char indices), and the selection after; whether it may be
/// made is [`format_blocked`]'s to say.
fn formatted(text: &str, sel: Range<usize>, f: Format) -> (String, Range<usize>) {
    match f {
        Format::Bold => wrap(text, sel, "**"),
        Format::Italic => wrap(text, sel, "*"),
        Format::Link => link(text, sel),
        Format::Heading | Format::Bullet | Format::Numbered | Format::Quote => {
            line_format(text, sel, f)
        }
    }
}

/// Why every format is off while the selection (or the cursor) is partly in a code span,
/// link, image or autolink: they are moved whole or not at all.
pub(super) const PARTLY_COVERS_SPAN: &str = "Selection partly covers a code span or link";

/// Why a format is off where the safety net stops it: the selection touches a block that
/// is not simple ([`obs_model::note_unformattable`]: code, raw HTML, a rule, a link
/// reference definition, a quote or heading inside a list or quote, a list nested deeper
/// than one level), or the change would show other text or formatting than asked for
/// ([`shows_as_intended`]).
pub(super) const FORMAT_NOT_HERE: &str = "This formatting can't be applied here";

/// Why `f` is off for the selection `sel` (char indices) of `text`, if it is; then
/// [`apply_format`] leaves the text as it is, byte for byte. A code span, link, image or
/// autolink ([`obs_model::NoteSpan::delimiter`] `None`) is opaque: a selection that touches
/// one without taking it in whole, or a cursor inside one, turns every format off
/// ([`PARTLY_COVERS_SPAN`]). One taken in whole is copied as typed. A format that would cut
/// one at a line break (Bold or Italic, a mark on each line; list, quote or heading, a block
/// boundary) is off too ([`SPAN_ACROSS_LINES`]), as is a link over lines. Past those, the
/// safety net: `f` is off ([`FORMAT_NOT_HERE`]) where the lines it acts on touch a block that
/// is not simple ([`touches_unformattable`]), or where its change, read again, does not show
/// as intended ([`shows_as_intended`]).
pub(super) fn format_blocked(text: &str, sel: Range<usize>, f: Format) -> Option<&'static str> {
    let (a, b) = (byte_at(text, sel.start), byte_at(text, sel.end));
    let spans = obs_model::note_spans(text);
    let partly = spans.iter().filter(|s| s.delimiter.is_none()).any(|s| {
        let r = &s.source;
        if a == b {
            r.start < a && a < r.end
        } else {
            r.start < b && a < r.end && !(a <= r.start && r.end <= b)
        }
    });
    if partly {
        return Some(PARTLY_COVERS_SPAN);
    }
    let why = match f {
        // A link is one line's text: a selection across lines cannot be one.
        Format::Link => text[a..b].contains('\n').then_some(LINK_ONE_LINE),
        // A mark goes on each selected line: one taken in whole over lines would be cut.
        Format::Bold | Format::Italic => spans
            .iter()
            .filter(|s| s.delimiter.is_none() && a < s.source.end && s.source.start < b)
            .any(|s| text[s.source.clone()].contains('\n'))
            .then_some(SPAN_ACROSS_LINES),
        _ => line_format_blocked(text, sel.clone(), f).then_some(SPAN_ACROSS_LINES),
    };
    if why.is_some() {
        return why;
    }
    if touches_unformattable(text, sel.clone(), f) {
        return Some(FORMAT_NOT_HERE);
    }
    let (new, r) = formatted(text, sel.clone(), f);
    let shows = if sel.is_empty() && matches!(f, Format::Bold | Format::Italic | Format::Link) {
        // Marks (or a link) with nothing in them yet, the cursor between: read as if a
        // character were typed there, as it will be.
        let typed = |t: &str, at: usize| {
            let at = byte_at(t, at);
            format!("{}x{}", &t[..at], &t[at..])
        };
        shows_as_intended(&typed(text, sel.start), &typed(&new, r.start), f)
    } else {
        shows_as_intended(text, &new, f)
            && lines_as_intended(text, sel, f, &new, r.clone())
            && marked_as_intended(&new, r, f)
    };
    (!shows).then_some(FORMAT_NOT_HERE)
}

/// Whether Bold or Italic left the selection `r` (char indices) of `new` all marked so, or
/// (taken off) none of it: each character it shows, white space aside. Delimiters next to
/// `*` or `_` typed as text can read otherwise: `**a` made italic reads as `**` then *a*.
fn marked_as_intended(new: &str, r: Range<usize>, f: Format) -> bool {
    let kind = match f {
        Format::Bold => NoteMarkKind::Bold,
        Format::Italic => NoteMarkKind::Italic,
        _ => return true,
    };
    let (a, b) = (byte_at(new, r.start), byte_at(new, r.end));
    let mut marked = Vec::new();
    for line in obs_model::parse_note_text(new) {
        let on = a.max(line.source.start)..b.min(line.source.end);
        if on.start >= on.end {
            continue;
        }
        let shown = line.text_range(on);
        for (k, c) in line.text[shown.clone()].char_indices() {
            if !c.is_whitespace() {
                let at = shown.start + k;
                marked.push(
                    line.marks
                        .iter()
                        .any(|m| m.kind == kind && m.range.contains(&at)),
                );
            }
        }
    }
    marked.iter().all(|m| *m) || marked.iter().all(|m| !*m)
}

/// Whether the lines a list, quote or heading change `f` on `sel` of `old` rewrote (`r` of
/// `new`, char indices) read, parsed again, as `f` asks: each one an item of the list, or
/// a quote, or none when `f` took it off (every line already was); the heading's line one
/// level smaller (`#` after text, text after `###`). A line can read otherwise: a line
/// taken out of a quote reads as the quote's text when it follows on from it.
fn lines_as_intended(old: &str, sel: Range<usize>, f: Format, new: &str, r: Range<usize>) -> bool {
    let is = |k: NoteLineKind| match f {
        Format::Bullet => matches!(k, NoteLineKind::Bullet { .. }),
        Format::Numbered => matches!(k, NoteLineKind::Numbered { .. }),
        Format::Quote => k == NoteLineKind::Quote,
        _ => false,
    };
    let before = obs_model::parse_note_text(old);
    let after = obs_model::parse_note_text(new);
    let (a, b) = (byte_at(new, r.start), byte_at(new, r.end));
    if f == Format::Heading {
        // A heading's kind is its first line's (a setext heading's underline is a gap).
        let line = |lines: &[obs_model::NoteLine], at: usize| {
            let l = lines
                .iter()
                .find(|l| l.source.start <= at && at <= l.source.end)?;
            let from = l.heading.as_ref().map_or(at, |h| h.source.start);
            lines
                .iter()
                .find(|l| l.source.start <= from && from <= l.source.end)
                .map(|l| l.kind)
        };
        let want = match line(&before, byte_at(old, sel.start)) {
            Some(NoteLineKind::Heading(n)) if n < 3 => NoteLineKind::Heading(n + 1),
            Some(NoteLineKind::Heading(_)) => NoteLineKind::Text,
            _ => NoteLineKind::Heading(1),
        };
        let got = line(&after, a);
        return got == Some(want)
            || (want == NoteLineKind::Text && got == Some(NoteLineKind::Blank));
    }
    if !matches!(f, Format::Bullet | Format::Numbered | Format::Quote) {
        return true;
    }
    let (first, last, _) = rewritten_lines(old, &before, sel, f);
    let off = before[first..=last].iter().all(|l| is(l.kind));
    after
        .iter()
        .filter(|l| l.kind != NoteLineKind::Blank && l.source.start <= b && a <= l.source.end)
        .all(|l| is(l.kind) != off)
}

/// Whether the lines `f` on `sel` (char indices) acts on touch a part of `text` that is not
/// a simple block ([`obs_model::note_unformattable`]): Bold, Italic and Link, each line the
/// selection touches; a list, quote or heading, the lines it rewrites.
fn touches_unformattable(text: &str, sel: Range<usize>, f: Format) -> bool {
    let unformattable = obs_model::note_unformattable(text);
    if unformattable.is_empty() {
        return false;
    }
    let lines: Vec<Range<usize>> = match f {
        Format::Bold | Format::Italic | Format::Link => {
            touched_lines(text, byte_at(text, sel.start), byte_at(text, sel.end))
        }
        _ => {
            let parsed = obs_model::parse_note_text(text);
            let (first, last, _) = rewritten_lines(text, &parsed, sel, f);
            parsed[first..=last]
                .iter()
                .map(|l| l.source.clone())
                .collect()
        }
    };
    lines.iter().any(|l| in_unformattable(&unformattable, l))
}

/// Whether the line `line` (bytes, without its line break) is in any of `unformattable`.
fn in_unformattable(unformattable: &[Range<usize>], line: &Range<usize>) -> bool {
    unformattable
        .iter()
        .any(|u| u.start < line.end.max(line.start + 1) && line.start < u.end)
}

/// Whether `new`, `f` made on `old`, shows what `f` asks for and nothing else, read again
/// by the parser ([`obs_model::note_shown`]): a list, quote or heading change shows the same
/// text, formatted the same (only its blocks and line breaks change); Bold, Italic or Link
/// the same but for bold, italic or a link with no target yet, in the same blocks
/// ([`obs_model::note_outline`]).
pub(super) fn shows_as_intended(old: &str, new: &str, f: Format) -> bool {
    use obs_model::NoteShownMark as M;
    let asked = |m: &M| match f {
        Format::Bold => *m == M::Bold,
        Format::Italic => *m == M::Italic,
        Format::Link => matches!(m, M::Link(t) if t.is_empty()),
        _ => false,
    };
    let inline = matches!(f, Format::Bold | Format::Italic | Format::Link);
    if obs_model::note_shown_except(old, asked) != obs_model::note_shown_except(new, asked) {
        return false;
    }
    if !inline {
        return true;
    }
    // The mark asked for goes on, or comes off, but not both: bold put on around text that
    // was bold and italic must not take its bold off.
    let has = |c: &(char, Vec<M>)| c.1.iter().any(asked);
    let (before, after) = (shown_chars(old), shown_chars(new));
    let gained = before.iter().zip(&after).any(|(o, n)| !has(o) && has(n));
    let lost = before.iter().zip(&after).any(|(o, n)| has(o) && !has(n));
    !(gained && lost) && obs_model::note_outline(old) == obs_model::note_outline(new)
}

/// The selected part of each line the selection `a..b` (bytes) touches, by line of `lines`
/// (the parse of `text`): past the line's heading, list or quote marker, its outer spaces
/// left outside, empty lines skipped. A part with an end inside a character reference
/// (`&#233;`) takes in the whole reference: it shows one character.
fn emphasis_parts(
    text: &str,
    lines: &[obs_model::NoteLine],
    a: usize,
    b: usize,
) -> Vec<(usize, Range<usize>)> {
    let mut parts = Vec::new();
    if a >= b {
        return parts;
    }
    for (i, line) in lines.iter().enumerate() {
        let (s, e) = (a.max(line.content), b.min(line.source.end));
        if s >= e {
            continue;
        }
        let part = &text[s..e];
        let (s, e) = (
            s + (part.len() - part.trim_start().len()),
            e - (part.len() - part.trim_end().len()),
        );
        if s < e {
            let whole = line.source_range(line.text_range(s..e));
            let (s, e) = if whole.is_empty() {
                (s, e)
            } else {
                (s.min(whole.start), e.max(whole.end))
            };
            parts.push((i, s..e));
        }
    }
    parts
}

/// Put `mark` around the selected part of each line ([`emphasis_parts`]), since a span
/// cannot run over a blank line or a list item's marker, or take it away when every part
/// already has it. Bold or italic that runs over a line break between those lines is first
/// cut there into one span per line ([`spans_cut_at`]), so each line's part is its own. A
/// part has the mark when a bold (or italic) span of the Note's CommonMark parse
/// ([`obs_model::note_spans`]) around or in it shows exactly the part's text: that span's own
/// delimiters (`**`/`__`, `*`/`_`) are what is taken away. With nothing selected the marks go
/// in with the cursor between them. A code span or link is never cut ([`format_blocked`]).
fn wrap(text: &str, sel: Range<usize>, mark: &str) -> (String, Range<usize>) {
    let (a, b) = (byte_at(text, sel.start), byte_at(text, sel.end));
    let lines = obs_model::parse_note_text(text);
    let parts = emphasis_parts(text, &lines, a, b);
    let breaks: Vec<usize> = match (parts.first(), parts.last()) {
        (Some((first, _)), Some((last, _))) => (*first..*last).collect(),
        _ => Vec::new(),
    };
    let edits = span_cuts(text, &lines, &breaks);
    if edits.is_empty() {
        return wrap_parts(text, &lines, parts, b, mark);
    }
    // The cut text, the selection moved past the delimiters put in before its ends.
    let shift = |x: usize, with_at: bool| {
        x + edits
            .iter()
            .filter(|(at, _)| *at < x || (with_at && *at == x))
            .map(|(_, d)| d.len())
            .sum::<usize>()
    };
    let (a, b) = (shift(a, false), shift(b, true));
    let cut = apply_edits(text, &edits);
    let lines = obs_model::parse_note_text(&cut);
    let parts = emphasis_parts(&cut, &lines, a, b);
    wrap_parts(&cut, &lines, parts, b, mark)
}

/// [`wrap`] on `parts` of `text` (`lines` its parse), or at the cursor `b` (bytes) when there
/// are none.
fn wrap_parts(
    text: &str,
    lines: &[obs_model::NoteLine],
    parts: Vec<(usize, Range<usize>)>,
    b: usize,
    mark: &str,
) -> (String, Range<usize>) {
    let n = mark.len();
    let spans = obs_model::note_spans(text);
    // The text `r` shows, line by line.
    let shown = |r: &Range<usize>| -> Vec<&str> {
        lines
            .iter()
            .filter_map(|l| {
                let on = r.start.max(l.source.start)..r.end.min(l.source.end);
                (on.start < on.end).then(|| &l.text[l.text_range(on)])
            })
            .filter(|s| !s.is_empty())
            .collect()
    };
    // The span marking part `p` so: where its `n` opening and `n` closing delimiter bytes
    // start, and the part of `p` inside them.
    let marked = |p: &Range<usize>| -> Option<(usize, usize, Range<usize>)> {
        let want = shown(p);
        if want.is_empty() {
            return None;
        }
        spans
            .iter()
            .find(|s| {
                s.delimiter.as_ref().is_some_and(|d| d.len() == n)
                    && s.source.len() >= 2 * n
                    && s.source.start < p.end
                    && p.start < s.source.end
                    && shown(&(s.source.start + n..s.source.end - n)) == want
            })
            .map(|s| {
                let (open, close) = (s.source.start, s.source.end - n);
                (
                    open,
                    close,
                    p.start.clamp(open + n, close)..p.end.clamp(open + n, close),
                )
            })
    };
    let mut parts: Vec<Range<usize>> = parts.into_iter().map(|(_, p)| p).collect();
    if parts.is_empty() {
        parts.push(b..b);
    }
    let found: Vec<_> = parts.iter().map(marked).collect();
    let remove = found.iter().all(Option::is_some);
    let mut new = String::with_capacity(text.len() + parts.len() * 2 * n);
    let mut at = 0;
    // Where the first and the last part are in `new`.
    let mut first = None;
    let mut last = 0;
    for (p, found) in parts.iter().zip(&found) {
        if let (true, Some((open, close, inner))) = (remove, found) {
            new.push_str(&text[at..*open]);
            new.push_str(&text[open + n..inner.start]);
            first.get_or_insert(new.len());
            new.push_str(&text[inner.clone()]);
            last = new.len();
            new.push_str(&text[inner.end..*close]);
            at = close + n;
            continue;
        }
        // A part that already has the marks keeps them when others get them.
        let add = found.is_none();
        new.push_str(&text[at..p.start]);
        at = p.end;
        if add {
            new.push_str(mark);
        }
        first.get_or_insert(new.len());
        new.push_str(&text[p.clone()]);
        last = new.len();
        if add {
            new.push_str(mark);
        }
    }
    new.push_str(&text[at..]);
    let chars = |s: &str| s.chars().count();
    let start = chars(&new[..first.unwrap_or(0)]);
    let end = chars(&new[..last]);
    (new, start..end)
}

/// `[selection](<…>)` with the cursor where the target goes (between `<` and `>`, so a
/// Bible reference's spaces are part of it); `[](<>)` with it in the brackets. Off for a
/// selection across lines ([`format_blocked`]): the toolbar keeps a link on one line.
fn link(text: &str, sel: Range<usize>) -> (String, Range<usize>) {
    let (a, b) = (byte_at(text, sel.start), byte_at(text, sel.end));
    let new = format!("{}[{}](<>)", &text[..a], &text[a..b]);
    let new = format!("{new}{}", &text[b..]);
    let c = if a == b { sel.start + 1 } else { sel.end + 4 };
    (new, c..c)
}

/// The lines the selection touches, as byte ranges.
fn touched_lines(text: &str, a: usize, b: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = text[..a].rfind('\n').map_or(0, |i| i + 1);
    loop {
        let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        out.push(start..end);
        if end >= b.max(start + 1) || end == text.len() {
            break;
        }
        start = end + 1;
    }
    out
}

/// The lines a list, quote or heading change `f` of `sel` (char indices) rewrites, as
/// `first..=last` of `lines` (the parse of `text`), and the line breaks around and between
/// them (by the line each comes after): a heading's lines all go together.
fn rewritten_lines(
    text: &str,
    lines: &[obs_model::NoteLine],
    sel: Range<usize>,
    f: Format,
) -> (usize, usize, Vec<usize>) {
    let a = byte_at(text, sel.start);
    let b = if f == Format::Heading {
        a
    } else {
        byte_at(text, sel.end)
    };
    let touched = touched_lines(text, a, b);
    let mut first = text[..touched[0].start].matches('\n').count();
    let mut last = first + touched.len() - 1;
    let same =
        |i: usize, j: usize| lines[i].heading.is_some() && lines[i].heading == lines[j].heading;
    while first > 0 && same(first - 1, first) {
        first -= 1;
    }
    while last + 1 < lines.len() && same(last + 1, last) {
        last += 1;
    }
    let breaks = (first.saturating_sub(1)..=last)
        .filter(|&i| i + 1 < lines.len())
        .collect();
    (first, last, breaks)
}

/// Why a change that would cut a code span or link at a line break is off: a mark on each
/// line (Bold, Italic) or a block of its own (list, quote, heading).
pub(super) const SPAN_ACROSS_LINES: &str = "Selection has a code span or link that crosses lines";

/// Whether list, quote or heading `f` on `sel` (char indices) would put a block boundary
/// inside a code span, link, image or autolink ([`obs_model::NoteSpan::delimiter`] `None`):
/// its parts would not show as the whole did, so the change is not made.
pub(super) fn line_format_blocked(text: &str, sel: Range<usize>, f: Format) -> bool {
    if !matches!(
        f,
        Format::Heading | Format::Bullet | Format::Numbered | Format::Quote
    ) {
        return false;
    }
    let lines = obs_model::parse_note_text(text);
    let (_, _, breaks) = rewritten_lines(text, &lines, sel, f);
    let spans = obs_model::note_spans(text);
    breaks.iter().any(|&i| {
        let at = lines[i].source.end;
        spans
            .iter()
            .any(|s| s.delimiter.is_none() && s.source.contains(&at))
    })
}

/// List or quote: on every line the selection touches, each read in its place in the Note
/// (a nested item is one under the item above it). Heading: on the line the selection
/// starts on (an ATX heading is one line). A heading touched anywhere is rewritten as a
/// whole, from the parse: its whole source (`#` markers, closing `#`s, a setext heading's
/// `===` or `---` underline) is replaced by its inline content, each line of it with the
/// new marker, so the shown text stays the same. Bold or italic over a line break that may
/// become a block boundary is cut there first ([`spans_cut_at`]), so each line keeps its
/// formatting; a code span, link, image or autolink over one leaves the text as it is
/// ([`format_blocked`]).
fn line_format(text: &str, sel: Range<usize>, f: Format) -> (String, Range<usize>) {
    let lines = obs_model::parse_note_text(text);
    let (first, last, breaks) = rewritten_lines(text, &lines, sel, f);
    let cut = spans_cut_at(text, &lines, &breaks);
    let (text, lines) = if cut == text {
        (text, lines)
    } else {
        let lines = obs_model::parse_note_text(&cut);
        (cut.as_str(), lines)
    };
    let chosen = &lines[first..=last];
    let all = |p: fn(NoteLineKind) -> bool| chosen.iter().all(|l| p(l.kind));
    let remove = match f {
        Format::Bullet => all(|k| matches!(k, NoteLineKind::Bullet { .. })),
        Format::Numbered => all(|k| matches!(k, NoteLineKind::Numbered { .. })),
        Format::Quote => all(|k| k == NoteLineKind::Quote),
        _ => false,
    };
    // The number the next numbered item at each level gets, and the shallowest level seen.
    let mut numbers: Vec<u32> = Vec::new();
    let mut shallowest: Option<u8> = None;
    // A selection's first item at a level follows on from a numbered item just above it.
    let number_above = |level: u8| {
        for l in lines[..first].iter().rev() {
            match l.kind {
                NoteLineKind::Numbered { level: at, number } if at == level => {
                    return number.saturating_add(1);
                }
                k if k.list_level().is_some_and(|at| at > level) => continue,
                _ => return 1,
            }
        }
        1
    };
    // The content column of the list item open at each level, as the lines are written: a
    // nested item goes under its parent's text, as CommonMark nests it (`- ` two columns,
    // `1. ` three).
    let mut cols = parent_columns(text, &lines[..first]);
    // The list item open at each level of the Note as it was: its content column then, and
    // now (`None` once it is no longer an item). The lines inside an item below the
    // selection are read against it ([`ItemColumns`]).
    let mut owners = ItemColumns(cols.iter().map(|&c| (c, Some(c))).collect());
    let mut out: Vec<String> = Vec::new();
    for l in chosen {
        let rest = match &l.heading {
            // A setext heading's underline: its lines get the new markers instead.
            Some(_) if !matches!(l.kind, NoteLineKind::Heading(_)) => continue,
            // This line's part of the heading's content.
            Some(h) => &text[l.content..l.source.end.min(h.content.end).max(l.content)],
            None => &text[l.content..l.source.end],
        };
        // Text's leading spaces are its own; a marker goes before what it says.
        let rest = match l.kind {
            NoteLineKind::Text if !remove => rest.trim_start(),
            _ => rest,
        };
        let level = l.kind.list_level().map_or(0, usize::from).min(cols.len());
        let marker = match f {
            _ if remove => String::new(),
            Format::Heading => match l.kind {
                NoteLineKind::Heading(n) if n < 3 => "#".repeat(usize::from(n) + 1) + " ",
                NoteLineKind::Heading(_) => String::new(),
                _ => "# ".into(),
            },
            Format::Bullet => "- ".into(),
            Format::Numbered => {
                let level = u8::try_from(level).unwrap_or(u8::MAX);
                let at = usize::from(level);
                numbers.truncate(at + 1);
                if numbers.len() <= at {
                    let start = if shallowest.is_none_or(|s| level <= s) {
                        number_above(level)
                    } else {
                        1
                    };
                    numbers.resize(at, 1);
                    numbers.push(start);
                }
                shallowest = Some(shallowest.map_or(level, |s| s.min(level)));
                let n = numbers[at];
                numbers[at] = n.saturating_add(1);
                format!("{n}. ")
            }
            _ => "> ".into(),
        };
        let rest = if f == Format::Heading && !marker.is_empty() {
            shown_hashes_escaped(rest)
        } else if l.heading.is_some() {
            block_start_escaped(rest)
        } else {
            rest.to_string()
        };
        let new_col = if matches!(f, Format::Bullet | Format::Numbered) && !remove {
            let indent = level.checked_sub(1).map_or(0, |p| cols[p]);
            cols.truncate(level);
            cols.push(indent + marker.len());
            out.push(format!("{}{marker}{rest}", " ".repeat(indent)));
            Some(indent + marker.len())
        } else {
            cols.clear();
            out.push(format!("{marker}{rest}"));
            None
        };
        match l.kind.list_level() {
            Some(old) => owners.open(old, l.content - l.source.start, new_col),
            // An item's own paragraphs and gaps are inside the list; other lines end it.
            None if l.kind == NoteLineKind::Blank
                || text[l.source.clone()].starts_with([' ', '\t']) => {}
            None => owners.0.clear(),
        }
    }
    let replaced = out.join("\n");
    // What is nested below the selection, written again under its (new) parents: nested
    // items, and an item's own paragraphs (and quotes, code) indented under its text, gaps
    // between them kept. When the last line stops being a list item, its nested items move
    // out a level: without an item above they would read as text.
    let still_list = !remove && matches!(f, Format::Bullet | Format::Numbered);
    let after = last + 1;
    let mut moved: Vec<String> = Vec::new();
    // Gaps are kept only when something nested follows them.
    let mut gap: Vec<String> = Vec::new();
    let mut shift: Option<usize> = None;
    for l in &lines[after..] {
        let line = &text[l.source.clone()];
        if l.kind == NoteLineKind::Blank {
            gap.push(line.to_string());
            continue;
        }
        let item = line.trim_start_matches([' ', '\t']);
        let written = match l.kind.list_level() {
            Some(0) => break,
            Some(old) => {
                let at = usize::from(old);
                let shift = *shift.get_or_insert(if still_list { 0 } else { at });
                let level = (at - shift.min(at)).min(cols.len());
                let marker_end = l.content.saturating_sub(l.source.end - item.len());
                let (marker, rest) = item.split_at(marker_end.min(item.len()));
                let indent = level.checked_sub(1).map_or(0, |p| cols[p]);
                cols.truncate(level);
                cols.push(indent + marker.len());
                owners.open(old, l.content - l.source.start, Some(indent + marker.len()));
                format!("{}{marker}{rest}", " ".repeat(indent))
            }
            None => {
                let indent = &line[..line.len() - item.len()];
                if indent.is_empty() || indent.contains('\t') {
                    break;
                }
                match owners.inside(indent.len()) {
                    None => break,
                    Some(None) => line.to_string(),
                    Some(Some(col)) => format!("{}{item}", " ".repeat(col)),
                }
            }
        };
        moved.append(&mut gap);
        moved.push(written);
    }
    let from = lines[first].source.start;
    let to = match moved.len() {
        0 => lines[last].source.end,
        k => lines[after + k - 1].source.end,
    };
    let mut tail = moved.iter().map(|m| format!("\n{m}")).collect::<String>();
    let new = format!("{}{replaced}{tail}{}", &text[..from], &text[to..]);
    // A line that is text now, over a `---` or `===` line, would make a setext heading of
    // it: a gap keeps them apart.
    let written = obs_model::parse_note_text(&new);
    let at = first + out.len() - 1;
    let setext = written[at].heading.is_some()
        && written
            .get(at + 1)
            .is_some_and(|l| l.heading == written[at].heading);
    if moved.is_empty() && setext {
        tail.push('\n');
    }
    let new = format!("{}{replaced}{tail}{}", &text[..from], &text[to..]);
    let start = text[..from].chars().count();
    (new, start..start + replaced.chars().count())
}

/// `text` with each bold or italic span ([`obs_model::note_spans`] with a delimiter) that
/// runs over one of the line `breaks` (the break after line `i` of `lines`, its parse)
/// closed at the end of that line and opened again where the next line's content starts,
/// each part with the span's delimiter. Inside a paragraph that shows the same; where the
/// break becomes a block boundary (a list item, quote or heading of its own), it keeps each
/// line's formatting, where the whole span would come apart into literal `*`s. A span is
/// cut only where it shows text on both sides of the break.
fn spans_cut_at(text: &str, lines: &[obs_model::NoteLine], breaks: &[usize]) -> String {
    apply_edits(text, &span_cuts(text, lines, breaks))
}

/// The delimiters [`spans_cut_at`] puts in: where, and which, by where.
fn span_cuts(text: &str, lines: &[obs_model::NoteLine], breaks: &[usize]) -> Vec<(usize, String)> {
    let spans: Vec<(Range<usize>, String)> = obs_model::note_spans(text)
        .into_iter()
        .filter_map(|s| Some((s.source, s.delimiter?)))
        .collect();
    // Whether `l` shows text of span `s`.
    let shows = |l: &obs_model::NoteLine, s: &Range<usize>| {
        let r = s.start.max(l.source.start)..s.end.min(l.source.end);
        r.start < r.end && !l.text_range(r).is_empty()
    };
    let mut edits: Vec<(usize, String)> = Vec::new();
    for &i in breaks {
        let (line, next) = (&lines[i], &lines[i + 1]);
        let crossing: Vec<&(Range<usize>, String)> = spans
            .iter()
            .filter(|(s, _)| {
                s.start < line.source.end
                    && next.source.start < s.end
                    && shows(line, s)
                    && shows(next, s)
            })
            .collect();
        let end = part_end(text, line);
        // Inner spans close first and open last.
        for (_, d) in crossing.iter().rev() {
            edits.push((end, d.clone()));
        }
        for (_, d) in &crossing {
            edits.push((next.content, d.clone()));
        }
    }
    edits.sort_by_key(|(at, _)| *at);
    edits
}

/// `text` with each of `edits` (by where) put in.
fn apply_edits(text: &str, edits: &[(usize, String)]) -> String {
    let mut out = String::with_capacity(text.len() + edits.len() * 2);
    let mut at = 0;
    for (to, d) in edits {
        out.push_str(&text[at..*to]);
        out.push_str(d);
        at = *to;
    }
    out.push_str(&text[at..]);
    out
}

/// Where a span's part on `line` ends: after its last character, before white space and a
/// hard break's `\` that are not shown (a closing `*` after a space would not close).
fn part_end(text: &str, line: &obs_model::NoteLine) -> usize {
    let bytes = text.as_bytes();
    let markup = |b: usize| line.text_range(b..b + 1).is_empty();
    let mut end = line.source.end;
    while end > line.content && matches!(bytes[end - 1], b' ' | b'\t' | b'\r') && markup(end - 1) {
        end -= 1;
    }
    if end > line.content && bytes[end - 1] == b'\\' && markup(end - 1) {
        end -= 1;
    }
    end
}

/// `rest`, shown text, made an ATX heading's content: `#`s ending it after a space (or
/// all of it) would read as the heading's closing markup, so the first is escaped and they
/// stay shown.
fn shown_hashes_escaped(rest: &str) -> String {
    let trimmed = rest.trim_end_matches([' ', '\t']);
    let before = trimmed.trim_end_matches('#');
    if before.len() < trimmed.len() && (before.is_empty() || before.ends_with([' ', '\t'])) {
        format!("{before}\\{}", &rest[before.len()..])
    } else {
        rest.to_string()
    }
}

/// `rest`, a heading's content, written where a line starts (as text, or after a list or
/// quote marker): escaped where it would start a block of its own there (`- `, `1. `, `>`,
/// a rule, a setext underline), so it shows as it did.
fn block_start_escaped(rest: &str) -> String {
    let alone = &obs_model::parse_note_text(rest)[0];
    let underline = rest.trim_end().bytes().all(|c| c == b'=');
    if rest.is_empty() || (alone.kind == NoteLineKind::Text && !underline) {
        return rest.to_string();
    }
    // A numbered item's `.` or `)`; anything else's first character.
    let at = rest.bytes().take_while(u8::is_ascii_digit).count();
    if rest[at..].starts_with(|c: char| c.is_ascii_punctuation()) {
        format!("{}\\{}", &rest[..at], &rest[at..])
    } else {
        rest.to_string()
    }
}

/// The list item open at each nesting level, as the Note was: its content column then, and
/// its content column as rewritten (`None` once it is no longer an item).
struct ItemColumns(Vec<(usize, Option<usize>)>);

impl ItemColumns {
    /// An item at `level`, its content at column `old` (now `new`): it closes the items
    /// nested deeper.
    fn open(&mut self, level: u8, old: usize, new: Option<usize>) {
        let level = usize::from(level);
        self.0.truncate(level);
        // A level with no item found: none of its lines are read against it.
        self.0.resize(level, (usize::MAX, None));
        self.0.push((old, new));
    }

    /// A line inside the list indented `indent` columns: in the deepest item whose content
    /// it reaches (closing those nested deeper), as CommonMark reads it. Where it goes now:
    /// the same distance past that item's content column, or `Some(None)` to stay as it is
    /// (its item is no longer one); `None` when it is in no item.
    fn inside(&mut self, indent: usize) -> Option<Option<usize>> {
        let at = self.0.iter().rposition(|(old, _)| *old <= indent)?;
        self.0.truncate(at + 1);
        let (old, new) = self.0[at];
        Some(new.map(|new| new + indent - old))
    }
}

/// The content columns of the list items the line after `above` would be nested under,
/// by level, read from the end of `above` (the Note's lines before it).
fn parent_columns(text: &str, above: &[obs_model::NoteLine]) -> Vec<usize> {
    let mut cols: Vec<Option<usize>> = Vec::new();
    let mut below: Option<u8> = None;
    for l in above.iter().rev() {
        match l.kind.list_level() {
            Some(level) if below.is_none_or(|b| level < b) => {
                let at = usize::from(level);
                if cols.len() <= at {
                    cols.resize(at + 1, None);
                }
                cols[at] = Some(l.content - l.source.start);
                below = Some(level);
                if level == 0 {
                    break;
                }
            }
            Some(_) => {}
            // An item's own paragraphs and gaps are inside the list; other lines end it.
            None if l.kind == NoteLineKind::Blank
                || text[l.source.clone()].starts_with([' ', '\t']) => {}
            None => break,
        }
    }
    // A level whose item was not found: two columns a level.
    let mut out = Vec::with_capacity(cols.len());
    for (i, c) in cols.into_iter().enumerate() {
        out.push(c.unwrap_or(2 * (i + 1)));
    }
    out
}

/// One format of the field's text: font, colour, slant, underline.
#[derive(Debug, Clone, PartialEq)]
struct Look {
    font: FontId,
    color: Color32,
    italics: bool,
    underline: bool,
}

/// The field's text as drawn (see the module docs), kept per field while the text is the same.
fn highlighted(ui: &Ui, id: egui::Id, text: &str) -> Arc<LayoutJob> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (text, ui.visuals().dark_mode).hash(&mut h);
    let key = h.finish();
    let cache = id.with("note-editor-look");
    if let Some((k, job)) = ui.data(|d| d.get_temp::<(u64, Arc<LayoutJob>)>(cache))
        && k == key
    {
        return job;
    }
    let job = Arc::new(layout_job(ui, text));
    ui.data_mut(|d| d.insert_temp(cache, (key, Arc::clone(&job))));
    job
}

fn layout_job(ui: &Ui, text: &str) -> LayoutJob {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let plain = Look {
        font: body.clone(),
        color: ui.visuals().text_color(),
        italics: false,
        underline: false,
    };
    let mut looks: Vec<Look> = vec![plain.clone(); text.len()];
    for bl in obs_engine::note_body(text) {
        let line = &bl.line;
        let src = line.source.clone();
        if let NoteLineKind::Heading(n) = line.kind {
            let size = body.size * [1.4, 1.25, 1.12].get(usize::from(n) - 1).unwrap_or(&1.05);
            for l in &mut looks[src.clone()] {
                l.font = theme::display(size);
                l.color = ui.visuals().strong_text_color();
            }
        }
        for m in &line.marks {
            let r = line.source_range(m.range.clone());
            for l in &mut looks[r] {
                match m.kind {
                    NoteMarkKind::Bold => l.font = theme::display(l.font.size),
                    NoteMarkKind::Italic => l.italics = true,
                    NoteMarkKind::Link(_) => {
                        l.color = theme::c().lapis;
                        l.underline = true;
                    }
                }
            }
        }
        for (r, _) in &bl.references {
            for l in &mut looks[line.source_range(r.clone())] {
                l.color = theme::c().lapis;
                l.underline = true;
            }
        }
        for r in line.markup() {
            for l in &mut looks[r] {
                l.color = theme::c().muted;
                l.underline = false;
            }
        }
    }
    let mut job = LayoutJob {
        keep_trailing_whitespace: true,
        ..LayoutJob::default()
    };
    let mut start = 0;
    for i in 1..=text.len() {
        if i == text.len() || (text.is_char_boundary(i) && looks[i] != looks[start]) {
            let look = &looks[start];
            job.append(
                &text[start..i],
                0.0,
                TextFormat {
                    font_id: look.font.clone(),
                    color: look.color,
                    italics: look.italics,
                    underline: if look.underline {
                        Stroke::new(1.0, look.color)
                    } else {
                        Stroke::NONE
                    },
                    ..TextFormat::default()
                },
            );
            start = i;
        }
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str, sub: &str) -> Range<usize> {
        let b = s.find(sub).unwrap();
        let start = s[..b].chars().count();
        start..start + sub.chars().count()
    }

    fn applied(text: &str, sel: &str, f: Format) -> (String, String) {
        let (new, r) = apply_format(text, chars(text, sel), f);
        let selected: String = new.chars().skip(r.start).take(r.len()).collect();
        (new, selected)
    }

    #[test]
    fn bold_and_italic_wrap_the_selection_and_toggle_off() {
        let (new, sel) = applied("by grace alone", "grace ", Format::Bold);
        assert_eq!(
            (new.as_str(), sel.as_str()),
            ("by **grace** alone", "grace")
        );
        let (new, sel) = apply_format(&new, chars(&new, "grace"), Format::Bold);
        assert_eq!(new, "by grace alone");
        assert_eq!(sel, chars("by grace alone", "grace"));
        let (new, _) = applied("faith", "faith", Format::Italic);
        assert_eq!(new, "*faith*");
        // Italic inside bold is added, not taken for the bold's marks.
        let (new, _) = applied("**faith**", "faith", Format::Italic);
        assert_eq!(new, "***faith***");
        // Nothing selected: the marks, with the cursor between them.
        let (new, r) = apply_format("ab", 1..1, Format::Bold);
        assert_eq!((new.as_str(), r), ("a****b", 3..3));
    }

    #[test]
    fn bold_and_italic_on_part_of_a_character_reference_take_in_the_whole_reference() {
        // Numeric (one char, two or four UTF-8 bytes) and named references, part selected:
        // the `#`, the digits, a letter of the name, or from inside one to the next word.
        for (text, sel, whole) in [
            ("&#233;", "#", "&#233;"),
            ("&#233;", "23", "&#233;"),
            ("a &eacute; x", "e", "&eacute;"),
            ("a &eacute; x", "acute; x", "&eacute; x"),
            ("a &#x1F600; b", "1F6", "&#x1F600;"),
            ("&amp;", "amp", "&amp;"),
        ] {
            for (f, mark) in [(Format::Bold, "**"), (Format::Italic, "*")] {
                let (new, _) = applied(text, sel, f);
                let want = text.replacen(whole, &format!("{mark}{whole}{mark}"), 1);
                assert_eq!(new, want, "{text:?} {sel:?} {f:?}");
                // Shown: the character, marked whole.
                let line = &obs_model::parse_note_text(&new)[0];
                assert_eq!(line.text, obs_model::note_plain_text(text), "{new:?}");
                assert_eq!(line.marks.len(), 1, "{new:?}");
                // And off again, from the same part of it.
                let (back, _) = applied(&new, sel, f);
                assert_eq!(back, text, "{new:?} {sel:?} {f:?}");
            }
        }
        // Inside a word, `**` before the `&` would read as literal (as before any
        // punctuation inside a word): read again, it would not show bold, so Bold is off
        // and the reference is never cut.
        let text = "caf&eacute;";
        assert_eq!(
            format_blocked(text, chars(text, "acu"), Format::Bold),
            Some(FORMAT_NOT_HERE)
        );
        assert_eq!(applied(text, "acu", Format::Bold).0, text);
    }

    #[test]
    fn line_formats_apply_to_every_line_touched_and_toggle_off() {
        let text = "intro\nfaith\nhope\nlove";
        let (new, _) = applied(text, "ith\nho", Format::Bullet);
        assert_eq!(new, "intro\n- faith\n- hope\nlove");
        let (new, _) = applied(&new, "faith\n- ho", Format::Bullet);
        assert_eq!(new, text);
        let (new, _) = applied(text, "faith\nhope\nlove", Format::Numbered);
        assert_eq!(new, "intro\n1. faith\n2. hope\n3. love");
        // A bulleted line made numbered keeps its text.
        let (new, _) = applied("- faith", "faith", Format::Numbered);
        assert_eq!(new, "1. faith");
        let (new, _) = applied(text, "hope", Format::Quote);
        assert_eq!(new, "intro\nfaith\n> hope\nlove");
        // Heading cycles #, ##, ### and back to text.
        let mut t = "Grace".to_string();
        for want in ["# Grace", "## Grace", "### Grace", "Grace"] {
            t = apply_format(&t, 0..0, Format::Heading).0;
            assert_eq!(t, want);
        }
    }

    #[test]
    fn heading_on_a_setext_heading_drops_its_underline_and_keeps_its_text() {
        // (kind, shown text) of each line that shows something.
        let shown = |t: &str| {
            obs_model::parse_note_text(t)
                .into_iter()
                .filter(|l| l.kind != NoteLineKind::Blank)
                .map(|l| (l.kind, l.text))
                .collect::<Vec<_>>()
        };
        let h = |n: u8, s: &str| (NoteLineKind::Heading(n), s.to_string());
        let text_line = |s: &str| (NoteLineKind::Text, s.to_string());
        // Cursor in the text, on the underline, or the whole heading selected.
        let text = "Grace\n===\nafter";
        for at in [2..2, 7..7, 0..5, 0..9] {
            let (new, r) = apply_format(text, at.clone(), Format::Heading);
            assert_eq!((new.as_str(), r), ("## Grace\nafter", 0..8), "{at:?}");
            assert_eq!(shown(&new), [h(2, "Grace"), text_line("after")]);
        }
        let (new, r) = apply_format("Grace\n---", 9..9, Format::Heading);
        assert_eq!((new.as_str(), r), ("### Grace", 0..9));
        assert_eq!(shown(&new), [h(3, "Grace")]);
        // A heading over lines: each line keeps its text, at the next level.
        let text = "intro\n\nby grace\nalone\n===";
        for at in [
            chars(text, "by"),
            chars(text, "alone"),
            chars(text, "by grace\nalone\n==="),
        ] {
            let (new, r) = apply_format(text, at, Format::Heading);
            assert_eq!(new, "intro\n\n## by grace\n## alone");
            assert_eq!(
                shown(&new),
                [text_line("intro"), h(2, "by grace"), h(2, "alone")]
            );
            let selected: String = new.chars().skip(r.start).take(r.len()).collect();
            assert_eq!(selected, "## by grace\n## alone");
        }
        // An ATX heading followed by a rule keeps the rule.
        let (new, _) = apply_format("# Grace\n---", 0..0, Format::Heading);
        assert_eq!(new, "## Grace\n---");
    }

    /// The non-empty text each line of `t` shows, and the levels of its headings.
    fn shown_and_levels(t: &str) -> (Vec<String>, Vec<u8>) {
        let lines = obs_model::parse_note_text(t);
        let shown = lines
            .iter()
            .filter(|l| !l.text.is_empty())
            .map(|l| l.text.clone())
            .collect();
        let levels = lines
            .iter()
            .filter_map(|l| match l.kind {
                NoteLineKind::Heading(n) => Some(n),
                _ => None,
            })
            .collect();
        (shown, levels)
    }

    /// Caret at the start, in the middle, at the end; the whole text selected.
    fn every_selection(t: &str) -> [Range<usize>; 4] {
        let n = t.chars().count();
        [0..0, n / 2..n / 2, n..n, 0..n]
    }

    #[test]
    fn heading_rewrites_every_commonmark_heading_form_from_its_parse() {
        // (Note, after Heading, the level of each of its headings after: none for text)
        let cases: &[(&str, &str, &[u8])] = &[
            ("# Grace", "## Grace", &[2]),
            ("## Grace", "### Grace", &[3]),
            ("### Grace", "Grace", &[]),
            ("#### Grace", "Grace", &[]),
            ("##### Grace", "Grace", &[]),
            ("###### Grace", "Grace", &[]),
            // Closing `#`s and trailing spaces are the heading's markup.
            ("### Grace ###", "Grace", &[]),
            ("# Grace #####", "## Grace", &[2]),
            ("## Grace ##   ", "### Grace", &[3]),
            ("# Grace   ", "## Grace", &[2]),
            ("# **Grace** #", "## **Grace**", &[2]),
            // Up to three leading spaces.
            ("   # Grace", "## Grace", &[2]),
            ("  ### Grace ###", "Grace", &[]),
            // Setext, over one line or more: each line gets the marker, the underline goes.
            ("Grace\n===", "## Grace", &[2]),
            ("Grace\n---", "### Grace", &[3]),
            ("   Grace\n  ===", "## Grace", &[2]),
            ("by grace\nalone\n===", "## by grace\n## alone", &[2, 2]),
            (
                "by *grace*\n  alone\n---",
                "### by *grace*\n### alone",
                &[3, 3],
            ),
            // Escaped `#`s stay text; `#`s shown as text are escaped where they would close.
            ("# Grace \\#", "## Grace \\#", &[2]),
            ("# \\# Grace", "## \\# Grace", &[2]),
            ("# C#", "## C#", &[2]),
            ("Grace #\n===", "## Grace \\#", &[2]),
            // Back to text, what would start a block is escaped.
            ("### \\# Grace", "\\# Grace", &[]),
            ("### 1. Grace", "1\\. Grace", &[]),
            ("### - Grace", "\\- Grace", &[]),
            ("### > Grace", "\\> Grace", &[]),
            ("### ===", "\\===", &[]),
            // A rule or `===` after a heading made text stays apart from it.
            ("### Grace\n---", "Grace\n\n---", &[]),
            ("### Grace\n===", "Grace\n\n===", &[]),
            ("intro\n### Grace\n---", "intro\nGrace\n\n---", &[]),
            // Empty headings.
            ("#", "## ", &[2]),
            ("# #", "## ", &[2]),
            ("### ###", "", &[]),
        ];
        for &(text, want, levels) in cases {
            let lines = obs_model::parse_note_text(text);
            let at = text
                .find(['#', 'G', 'b'])
                .map_or(0, |b| text[..b].chars().count());
            for sel in every_selection(text)
                .into_iter()
                .chain(std::iter::once(at..at))
                // On the heading only: the lines around it are not changed here.
                .filter(|s| {
                    let line = |c: usize| text.chars().take(c).filter(|&c| c == '\n').count();
                    (line(s.start)..=line(s.end)).all(|i| lines[i].heading.is_some())
                })
            {
                let (new, r) = apply_format(text, sel.clone(), Format::Heading);
                assert_eq!(new, want, "{text:?} at {sel:?}");
                let (shown, now) = shown_and_levels(&new);
                assert_eq!(shown, shown_and_levels(text).0, "{text:?}");
                assert_eq!(now, levels, "{text:?}");
                assert!(r.end <= new.chars().count());
            }
        }
    }

    #[test]
    fn list_and_quote_on_a_heading_take_its_content_from_the_parse() {
        let cases = [
            ("### Grace ###", Format::Bullet, "- Grace"),
            ("## *Grace* ##", Format::Numbered, "1. *Grace*"),
            ("## *Grace* ##", Format::Quote, "> *Grace*"),
            ("Grace\n===", Format::Bullet, "- Grace"),
            ("by grace\nalone\n===", Format::Quote, "> by grace\n> alone"),
            (
                "by grace\nalone\n---",
                Format::Bullet,
                "- by grace\n- alone",
            ),
            ("### 1. Grace", Format::Bullet, "- 1\\. Grace"),
            ("# > Grace", Format::Quote, "> \\> Grace"),
        ];
        for (text, f, want) in cases {
            for sel in every_selection(text) {
                let (new, _) = apply_format(text, sel.clone(), f);
                assert_eq!(new, want, "{text:?} {f:?} at {sel:?}");
                let (shown, levels) = shown_and_levels(&new);
                assert_eq!(shown, shown_and_levels(text).0, "{text:?} {f:?}");
                assert!(levels.is_empty(), "{text:?} {f:?}");
            }
        }
    }

    #[test]
    fn bold_and_italic_mark_each_line_of_a_selection_past_its_marker() {
        // Each line gets its own marks (a span cannot cross a list marker or blank line).
        let (new, sel) = applied("faith\nhope", "faith\nhope", Format::Bold);
        assert_eq!(
            (new.as_str(), sel.as_str()),
            ("**faith**\n**hope**", "faith**\n**hope")
        );
        let lines = obs_model::parse_note_text(&new);
        assert!(lines.iter().all(|l| l.marks.len() == 1
            && l.marks[0].kind == NoteMarkKind::Bold
            && l.marks[0].range == (0..l.text.len())));
        let (new, _) = applied(&new, "faith**\n**hope", Format::Bold);
        assert_eq!(new, "faith\nhope");
        // Past each line's list, heading or quote marker; blank lines are left alone.
        let text = "## Grace\n- faith\n  1. hope\n\n> love";
        let (new, _) = applied(text, text, Format::Italic);
        assert_eq!(new, "## *Grace*\n- *faith*\n  1. *hope*\n\n> *love*");
        for l in obs_model::parse_note_text(&new) {
            assert!(
                l.kind == NoteLineKind::Blank
                    || l.marks
                        .iter()
                        .any(|m| m.kind == NoteMarkKind::Italic && m.range == (0..l.text.len())),
                "{l:?}"
            );
        }
        let (back, _) = applied(&new, &new, Format::Italic);
        assert_eq!(back, text);
        // A line already bold keeps its marks while the others get theirs.
        let (new, _) = applied("**faith**\nhope", "**faith**\nhope", Format::Bold);
        assert_eq!(new, "**faith**\n**hope**");
        let (new, _) = applied(&new, "**faith**\n**hope**", Format::Bold);
        assert_eq!(new, "faith\nhope");
        // Italic over bold-italic takes only the italic away.
        let (new, _) = applied("***faith***", "***faith***", Format::Italic);
        assert_eq!(new, "**faith**");
    }

    #[test]
    fn bold_and_italic_written_with_underscores_toggle_off_too() {
        for (text, sel, f, want) in [
            (
                "by __faith__ alone",
                "faith",
                Format::Bold,
                "by faith alone",
            ),
            (
                "by _faith_ alone",
                "faith",
                Format::Italic,
                "by faith alone",
            ),
            ("__faith__", "__faith__", Format::Bold, "faith"),
            ("_faith_", "_faith_", Format::Italic, "faith"),
            // The italic outside a bold, and the bold outside an italic.
            ("_**faith**_", "faith", Format::Italic, "**faith**"),
            ("__*faith*__", "faith", Format::Bold, "*faith*"),
            ("*__faith__*", "faith", Format::Bold, "*faith*"),
            ("**_faith_**", "faith", Format::Italic, "**faith**"),
            ("___faith___", "faith", Format::Italic, "__faith__"),
            // Each line, however it is written.
            (
                "__faith__\n**hope**",
                "faith__\n**hope",
                Format::Bold,
                "faith\nhope",
            ),
            // Not emphasis: underscores inside a word are left, and the mark goes on.
            (
                "snake_case_name",
                "case",
                Format::Italic,
                "snake_*case*_name",
            ),
        ] {
            let (new, selected) = applied(text, sel, f);
            assert_eq!(new, want, "{text:?} {f:?}");
            if new.len() < text.len() {
                assert_eq!(selected, sel.replace(['*', '_'], ""), "{text:?}");
            }
        }
        // Pressed again, the mark goes back on (with `*`).
        let (new, _) = applied("by faith alone", "faith", Format::Bold);
        assert_eq!(new, "by **faith** alone");
    }

    #[test]
    fn line_formats_read_nested_items_in_their_place() {
        // Both items bulleted: the bullet comes off both, the nested one included.
        let (new, _) = applied("- a\n  - b", "a\n  - b", Format::Bullet);
        assert_eq!(new, "a\nb");
        // Numbered: each level numbers from 1, the nesting kept.
        let (new, _) = applied(
            "- a\n  - b\n  - c\n- d",
            "a\n  - b\n  - c\n- d",
            Format::Numbered,
        );
        assert_eq!(new, "1. a\n   1. b\n   2. c\n2. d");
        let kinds: Vec<_> = obs_model::parse_note_text(&new)
            .iter()
            .map(|l| l.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                NoteLineKind::Numbered {
                    level: 0,
                    number: 1
                },
                NoteLineKind::Numbered {
                    level: 1,
                    number: 1
                },
                NoteLineKind::Numbered {
                    level: 1,
                    number: 2
                },
                NoteLineKind::Numbered {
                    level: 0,
                    number: 2
                },
            ]
        );
        let (back, _) = applied(&new, &new, Format::Numbered);
        assert_eq!(back, "a\nb\nc\nd");
        // A parent numbered: its nested items move under its text, so they stay nested.
        let (new, _) = applied("- a\n  - b\n- c", "a", Format::Numbered);
        assert_eq!(new, "1. a\n   - b\n- c");
        assert_eq!(
            obs_model::parse_note_text(&new)[1].kind,
            NoteLineKind::Bullet { level: 1 }
        );
        // An item made under a numbered one goes under its text.
        let (new, _) = applied("1. x\ny", "y", Format::Bullet);
        assert_eq!(new, "1. x\n- y");
        let (new, _) = applied("1. x\n   - y\n   - z", "z", Format::Numbered);
        assert_eq!(new, "1. x\n   - y\n   1. z");
        assert_eq!(
            obs_model::parse_note_text(&new)[2].kind,
            NoteLineKind::Numbered {
                level: 1,
                number: 1
            }
        );
        // A line numbered under a numbered list follows on from its number.
        let (new, _) = applied("1. x\n   - y\nz", "z", Format::Numbered);
        assert_eq!(new, "1. x\n   - y\n2. z");
        // A parent that stops being an item: its nested items move out, not to text.
        let text = "- a\n  - b\n  - c\n- d";
        let (new, _) = applied(text, "a", Format::Bullet);
        assert_eq!(new, "a\n- b\n- c\n- d");
        // Nested deeper than one level, the list is not a simple block: off.
        let text = "- a\n  - b\n    - c\n  - d\n- e";
        assert_eq!(applied(text, "a", Format::Bullet).0, text);
        let (new, _) = applied("- a\n  - b", "a", Format::Quote);
        assert_eq!(new, "> a\n- b");
        let levels: Vec<_> = obs_model::parse_note_text(&new)
            .iter()
            .map(|l| l.kind)
            .collect();
        assert_eq!(
            levels,
            [NoteLineKind::Quote, NoteLineKind::Bullet { level: 0 }]
        );
    }

    #[test]
    fn line_formats_keep_what_is_nested_below_a_gap_under_its_item() {
        // (kind, shown text) of each line that shows something.
        let shown = |t: &str| {
            obs_model::parse_note_text(t)
                .into_iter()
                .filter(|l| l.kind != NoteLineKind::Blank)
                .map(|l| (l.kind, l.text))
                .collect::<Vec<_>>()
        };
        let numbered = |level, number| NoteLineKind::Numbered { level, number };
        // A nested item after a gap moves under the numbered parent's text.
        let (new, _) = applied("- a\n\n  - b", "a", Format::Numbered);
        assert_eq!(new, "1. a\n\n   - b");
        assert_eq!(
            shown(&new),
            [
                (numbered(0, 1), "a".to_string()),
                (NoteLineKind::Bullet { level: 1 }, "b".to_string()),
            ]
        );
        // An item's own paragraph after a gap, and an item nested below it, stay in it.
        let text = "- a\n\n  more\n\n  - b\n\n    deeper\n\nafter";
        let (new, _) = applied(text, "a", Format::Numbered);
        assert_eq!(new, "1. a\n\n   more\n\n   - b\n\n     deeper\n\nafter");
        // The same blocks, in the same items, as the HTML export reads them.
        let html = |t: &str| {
            obs_engine::notes_export::note_html(t)
                .replace("<ol>", "<ul>")
                .replace("</ol>", "</ul>")
        };
        assert_eq!(html(&new), html(text));
        // Back to bullets: the same nesting.
        let (back, _) = applied(&new, "a", Format::Bullet);
        assert_eq!(back, text);
        // A gap before a top-level item or text: nothing below is moved.
        let (new, _) = applied("- a\n\n- b\n\n  c", "a", Format::Numbered);
        assert_eq!(new, "1. a\n\n- b\n\n  c");
        // A parent that stops being an item: its nested item after a gap moves out a level.
        let (new, _) = applied("- a\n\n  - b", "a", Format::Quote);
        assert_eq!(new, "> a\n\n- b");
        assert_eq!(shown(&new)[1].0, NoteLineKind::Bullet { level: 0 });
        // And its own paragraph moves out with it.
        let (new, _) = applied("- a\n  - b\n\n    more", "a", Format::Quote);
        assert_eq!(new, "> a\n- b\n\n  more");
    }

    /// A line's shown text and its formatting (what, over which bytes).
    type Formatted = (String, Vec<(Range<usize>, NoteMarkKind)>);

    /// Each line that shows text, formatted.
    fn shown_formatting(t: &str) -> Vec<Formatted> {
        obs_model::parse_note_text(t)
            .into_iter()
            .filter(|l| !l.text.is_empty())
            .map(|l| {
                let marks = l.marks.into_iter().map(|m| (m.range, m.kind)).collect();
                (l.text, marks)
            })
            .collect()
    }

    #[test]
    fn line_formats_cut_bold_and_italic_over_lines_so_each_line_keeps_its_formatting() {
        // (Note, selection, format, after): every line shows the same text, formatted the
        // same; only the blocks change.
        let all = |t: &str| 0..t.chars().count();
        let bold = "**faith\nhope**";
        let cases: &[(&str, Range<usize>, Format, &str)] = &[
            (bold, all(bold), Format::Bullet, "- **faith**\n- **hope**"),
            (
                bold,
                all(bold),
                Format::Numbered,
                "1. **faith**\n2. **hope**",
            ),
            (bold, all(bold), Format::Quote, "> **faith**\n> **hope**"),
            // Heading: the line the selection starts on.
            (bold, all(bold), Format::Heading, "# **faith**\n**hope**"),
            (bold, 0..0, Format::Heading, "# **faith**\n**hope**"),
            (bold, 10..10, Format::Heading, "**faith**\n# **hope**"),
            ("*a\nb*", 0..5, Format::Bullet, "- *a*\n- *b*"),
            ("*a\nb*", 0..5, Format::Quote, "> *a*\n> *b*"),
            ("*a\nb*", 0..0, Format::Heading, "# *a*\n*b*"),
            ("__a\nb__", 0..7, Format::Numbered, "1. __a__\n2. __b__"),
            (
                "**a *b\nc* d**",
                0..14,
                Format::Bullet,
                "- **a *b***\n- ***c* d**",
            ),
            (
                "**a *b\nc* d**",
                0..14,
                Format::Quote,
                "> **a *b***\n> ***c* d**",
            ),
            (
                "**a *b\nc* d**",
                0..0,
                Format::Heading,
                "# **a *b***\n***c* d**",
            ),
            // Only the lines rewritten are cut from those around them.
            ("*a\nb\nc*", 3..3, Format::Bullet, "*a*\n- *b*\n*c*"),
            // Spaces before the break stay outside, where they are not shown.
            ("*a  \nb*", 0..7, Format::Bullet, "- *a*  \n- *b*"),
            // A heading over lines: each line keeps its formatting at the next level.
            (
                "*faith\nhope*\n===",
                2..2,
                Format::Heading,
                "## *faith*\n## *hope*",
            ),
            (
                "*faith\nhope*\n---",
                9..9,
                Format::Bullet,
                "- *faith*\n- *hope*",
            ),
        ];
        for (text, sel, f, want) in cases {
            let (new, r) = apply_format(text, sel.clone(), *f);
            assert_eq!(new, *want, "{text:?} {f:?} at {sel:?}");
            assert_eq!(
                shown_formatting(&new),
                shown_formatting(text),
                "{text:?} {f:?}"
            );
            assert!(r.end <= new.chars().count());
        }
    }

    #[test]
    fn line_formats_leave_a_code_span_or_link_over_lines_as_it_is() {
        // A block boundary inside a code span, link, image or autolink would change what it
        // shows (a code span's stripped spaces, a link's target): the change is off, and
        // the Note's bytes stay as they are (#186 review).
        let all = |t: &str| 0..t.chars().count();
        let blocked = [
            "` a\nb `",
            "`a\nb`",
            "`` `a\nb` ``",
            "[x\ny](u)",
            "[x\ny](<Rom 5:8> \"t\")",
            "[x\ny]\n\n[x y]: <https://example.org/?q=\\&amp;>",
            "![a\nb](u)",
            "*[x\ny](u)*",
            "**a `b\nc` d**",
        ];
        for text in blocked {
            let to = text.find("\n\n").map_or(all(text), |i| 0..i);
            for f in [Format::Bullet, Format::Numbered, Format::Quote] {
                assert!(line_format_blocked(text, to.clone(), f), "{text:?} {f:?}");
                let (new, r) = apply_format(text, to.clone(), f);
                assert_eq!(new, text, "{f:?}");
                assert_eq!(r, to, "{text:?} {f:?}");
            }
            // Heading: the line it starts on, its break into the next inside the span.
            assert!(line_format_blocked(text, 0..0, Format::Heading), "{text:?}");
            assert_eq!(apply_format(text, 0..0, Format::Heading).0, text);
        }
        // The span's last line alone: a block boundary before it splits the span too.
        let text = "a\n`b\nc`";
        for f in [Format::Bullet, Format::Quote, Format::Heading] {
            assert!(line_format_blocked(text, 6..6, f), "{f:?}");
            assert_eq!(apply_format(text, 6..6, f).0, text);
        }
        // The line above it: it stays one paragraph's text, so the change is made.
        assert_eq!(apply_format(text, 0..0, Format::Bullet).0, "- a\n`b\nc`");
        // A code span or link on one line, or one the change keeps inside a paragraph, is fine.
        let one_line = "`a b` [x](u)\nc";
        assert!(!line_format_blocked(
            one_line,
            all(one_line),
            Format::Bullet
        ));
        assert_eq!(
            apply_format(one_line, all(one_line), Format::Bullet).0,
            "- `a b` [x](u)\n- c"
        );
        let whole = "`a\nb`";
        assert!(!line_format_blocked(
            &format!("{whole}\n\nc"),
            7..7,
            Format::Bullet
        ));
        // Bold and italic put no block boundary anywhere; they are off over it because a
        // mark would go on each line ([`format_blocked`]).
        assert!(!line_format_blocked("`a\nb`", 0..5, Format::Bold));
        assert_eq!(
            format_blocked("`a\nb`", 0..5, Format::Bold),
            Some(SPAN_ACROSS_LINES)
        );
    }

    #[test]
    fn formats_act_only_on_simple_blocks() {
        // The safety net (#186 review): a code block, raw HTML, a rule, a link reference
        // definition, a quote or heading inside a list or quote, or a list nested deeper
        // than one level, reads its lines in ways a marker put in or taken out could
        // change; every format that touches one is off and the Note stays as it is.
        let off = Some(FORMAT_NOT_HERE);
        // Review repro: the item's text is an indented code block showing `**faith**` as
        // typed; a `1. ` marker in place of `- ` would make it bold `faith`.
        let text = "-     **faith**";
        assert_eq!(format_blocked(text, 3..3, Format::Numbered), off);
        assert_eq!(
            apply_format(text, 3..3, Format::Numbered),
            (text.into(), 3..3)
        );
        let line = obs_model::parse_note_text(text).remove(0);
        assert_eq!((line.text.as_str(), line.marks.len()), ("**faith**", 0));
        // (Note, selection, format): each off, the Note left byte for byte.
        let cases: &[(&str, Range<usize>, Format)] = &[
            ("-     **faith**", 3..3, Format::Bullet),
            ("-     **faith**", 3..3, Format::Quote),
            ("-     **faith**", 3..3, Format::Heading),
            ("    **faith**", 0..0, Format::Bullet),
            ("    **faith**", 0..0, Format::Quote),
            // Read again, the text above becoming an item would take the code in as the
            // item's paragraph.
            ("a\n\n    **x**", 0..0, Format::Bullet),
            ("a\n\n    **x**", 0..0, Format::Numbered),
            // Fences are lines of their own: a marker on one ends the block.
            ("```\n**x**\n```", 0..0, Format::Bullet),
            ("```\n**x**\n```", 0..13, Format::Numbered),
            ("```\n**x**\n```", 4..4, Format::Heading),
            // Items and quotes holding code.
            ("- a\n  ```\n  **x**\n  ```", 2..2, Format::Numbered),
            ("- a\n\n      **x**", 2..2, Format::Numbered),
            ("- a\n\n      **x**", 2..2, Format::Quote),
            ("> a\n>\n>     **x**", 0..0, Format::Bullet),
            // A mark or link typed in code shows as typed: its text would change.
            ("```\n**x**\n```", 4..9, Format::Bold),
            ("    faith", 4..9, Format::Italic),
            ("    faith", 6..6, Format::Bold),
            ("    faith", 4..9, Format::Link),
            // Raw HTML, a rule, a link reference definition.
            ("<div>\nfaith\n</div>", 6..11, Format::Bold),
            ("<div>\nfaith\n</div>", 6..6, Format::Bullet),
            ("faith\n\n***", 7..7, Format::Heading),
            ("[x]\n\n[x]: u", 5..5, Format::Quote),
            // A list in a quote, a quote or heading in an item, a list three levels deep.
            ("> - faith", 4..9, Format::Bold),
            ("> - faith", 4..4, Format::Numbered),
            ("- > faith", 4..4, Format::Bullet),
            ("- # faith", 4..9, Format::Italic),
            ("- a\n  - b\n    - c", 0..0, Format::Bullet),
            ("- a\n  - b\n    - c", 18..18, Format::Quote),
        ];
        for (text, sel, f) in cases {
            assert_eq!(format_blocked(text, sel.clone(), *f), off, "{text:?} {f:?}");
            assert_eq!(
                apply_format(text, sel.clone(), *f),
                (text.to_string(), sel.clone()),
                "{text:?} {f:?}"
            );
        }
        // Outside those blocks the formats work: a paragraph above code, a list one level
        // deep with paragraphs.
        for (text, sel, f, want) in [
            ("a\n\n    **x**", 0..1, Format::Bold, "**a**\n\n    **x**"),
            ("- a\n  - b", 8..8, Format::Numbered, "- a\n  1. b"),
            ("- a\n\n  more", 2..2, Format::Numbered, "1. a\n\n   more"),
        ] {
            assert_eq!(format_blocked(text, sel.clone(), f), None, "{text:?} {f:?}");
            let (new, _) = apply_format(text, sel, f);
            assert_eq!(new, want, "{text:?} {f:?}");
        }
    }

    #[test]
    fn a_code_span_or_link_is_moved_whole_or_every_format_is_off() {
        // A code span, link, image or autolink is opaque: no format cuts one or puts a mark
        // inside it (#186 review). `(Note, selection, why off)`: the text stays byte for byte.
        let all = |t: &str| 0..t.chars().count();
        let partly = Some(PARTLY_COVERS_SPAN);
        let across = Some(SPAN_ACROSS_LINES);
        let every = [
            Format::Bold,
            Format::Italic,
            Format::Heading,
            Format::Bullet,
            Format::Numbered,
            Format::Quote,
            Format::Link,
        ];
        let cases: &[(&str, Range<usize>, Format, Option<&str>)] = &[
            // Review repro: a mark on each line would land inside the code span.
            ("`a\nb`", 0..5, Format::Bold, across),
            ("`a\nb`", 0..5, Format::Italic, across),
            ("> `a\n> b`", 0..9, Format::Bold, across),
            ("[x\ny](u)", 0..8, Format::Italic, across),
            ("**a `b\nc` d**", 0..14, Format::Bold, across),
        ];
        for (text, sel, f, why) in cases {
            assert_eq!(
                format_blocked(text, sel.clone(), *f),
                *why,
                "{text:?} {f:?}"
            );
            assert_eq!(
                apply_format(text, sel.clone(), *f),
                (text.to_string(), sel.clone())
            );
        }
        // Partly covered (review repro: a code span whose stripped edge spaces moved its
        // indent), or the cursor inside one: every format is off.
        for (text, sel) in [
            ("` \n hope `", 4..8),
            ("` \n hope `", 0..4),
            ("see `a b` now", 7..13),
            ("see `a b` now", 6..6),
            ("[x\ny](u)", 1..4),
            ("[faith](<Rom 5:8>) now", 0..5),
            ("<https://a.org> x", 4..4),
            ("![a](u) b", 2..9),
        ] {
            for f in every {
                assert_eq!(
                    format_blocked(text, sel.clone(), f),
                    partly,
                    "{text:?} {f:?}"
                );
                assert_eq!(apply_format(text, sel.clone(), f).0, text, "{text:?} {f:?}");
            }
        }
        // Taken in whole, on one line, it is copied as typed and the format is made.
        for (text, f, want) in [
            ("see `a b` now", Format::Bold, "**see `a b` now**"),
            ("x [a](<Rom 5:8>) y", Format::Italic, "*x [a](<Rom 5:8>) y*"),
            ("`a b`\nc", Format::Bold, "**`a b`**\n**c**"),
            ("`a b`\nc", Format::Bullet, "- `a b`\n- c"),
            ("`a b`", Format::Link, "[`a b`](<>)"),
        ] {
            assert_eq!(format_blocked(text, all(text), f), None, "{text:?} {f:?}");
            let (new, r) = apply_format(text, all(text), f);
            assert_eq!(new, want, "{text:?} {f:?}");
            if f != Format::Link {
                assert_eq!(apply_format(&new, r, f).0, text, "{text:?} {f:?}");
            }
        }
        // At its edges, the cursor is not in it.
        assert_eq!(format_blocked("`ab`", 0..0, Format::Bold), None);
        assert_eq!(format_blocked("`ab`", 4..4, Format::Bold), None);
    }

    #[test]
    fn bold_and_italic_over_lines_cut_a_span_over_them_into_one_per_line() {
        // Bold or italic that runs over the lines is cut at each break first, so each line's
        // part is its own: marked on each line, or taken off each (#186 review).
        for (text, f, want) in [
            ("**faith\nhope**", Format::Bold, "faith\nhope"),
            ("*a\nb*", Format::Bold, "***a***\n***b***"),
            ("*a\nb*", Format::Italic, "a\nb"),
            ("__a\nb__", Format::Bold, "a\nb"),
            ("> **a\n> b**", Format::Bold, "> a\n> b"),
        ] {
            let sel = 0..text.chars().count();
            let (new, r) = apply_format(text, sel.clone(), f);
            assert_eq!(new, want, "{text:?} {f:?}");
            let kind = if f == Format::Bold {
                NoteMarkKind::Bold
            } else {
                NoteMarkKind::Italic
            };
            let added = new.len() > text.len();
            for (line, marks) in shown_formatting(&new) {
                let whole = marks
                    .iter()
                    .any(|(m, k)| *k == kind && *m == (0..line.len()));
                assert_eq!(whole, added, "{text:?} {f:?}: {line:?} {marks:?}");
            }
            assert!(r.end <= new.chars().count());
        }
        // The selected text of the bold, too.
        let (new, r) = apply_format("**faith\nhope**", 2..12, Format::Bold);
        assert_eq!((new.as_str(), r), ("faith\nhope", 0..10));
    }

    #[test]
    fn link_puts_the_cursor_where_the_target_goes() {
        let (new, r) = apply_format("see this", chars("see this", "this"), Format::Link);
        assert_eq!(new, "see [this](<>)");
        assert_eq!(r, 12..12);
        let (new, r) = apply_format("x", 1..1, Format::Link);
        assert_eq!((new.as_str(), r), ("x[](<>)", 2..2));
        // Typed between `<` and `>`, a Bible reference with its spaces is the target.
        let typed = "see [this](<Rom 5:8>)";
        assert_eq!(
            obs_model::parse_note_text(typed)[0].marks[0].kind,
            NoteMarkKind::Link("Rom 5:8".into())
        );
        // The toolbar keeps a link on one line.
        let (new, r) = apply_format("faith\nhope", 0..10, Format::Link);
        assert_eq!((new.as_str(), r), ("faith\nhope", 0..10));
    }

    #[test]
    fn enter_continues_a_list_and_ends_it_in_an_empty_item() {
        let text = "- faith";
        let (new, r) = list_continuation(text, 7).unwrap();
        assert_eq!((new.as_str(), r), ("- faith\n- ", 10..10));
        let (new, r) = list_continuation(&new, 10).unwrap();
        assert_eq!((new.as_str(), r), ("- faith\n", 8..8));
        let (new, _) = list_continuation("  9) nine", 9).unwrap();
        assert_eq!(new, "  9) nine\n  10) ");
        let (new, _) = list_continuation("> quoted", 8).unwrap();
        assert_eq!(new, "> quoted\n> ");
        // Not in a list, or before the marker: Enter is Enter.
        assert!(list_continuation("text", 4).is_none());
        assert!(list_continuation("- faith", 1).is_none());
        // The parse decides what is a list: an indented code line is not one, and a list
        // item's marker is kept as typed.
        assert!(list_continuation("    - code", 10).is_none());
        assert!(list_continuation("`- a`", 5).is_none());
        // A list in a quote is not a simple block: Enter is the field's own.
        assert!(list_continuation("> - faith", 9).is_none());
        let (new, _) = list_continuation("* faith", 7).unwrap();
        assert_eq!(new, "* faith\n* ");
    }
}

#[cfg(test)]
#[path = "note_editor_corpus_tests.rs"]
mod corpus_tests;
