//! The Note editor (#186): a text field for a Note's text with Note formatting, used by the
//! Notes Tool's cards and the popup Note editor.
//!
//! - **Toolbar** above the field: **B**old, *I*talic, Heading, bulleted list, numbered list,
//!   block quote and link. Each acts on the selection (or the lines it touches) and keeps the
//!   keys in the field; pressing one does not end the editing pass. Bold and italic mark each
//!   selected line on its own, since they end with their line; a link does too, so the link
//!   button is off while the selection spans lines.
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

use crate::theme;

/// A change the toolbar or a shortcut makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    Bold,
    Italic,
    /// Cycles the lines through `#`, `##`, `###` and back to text.
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

    fn label(self) -> RichText {
        match self {
            Format::Bold => RichText::new("B").strong(),
            Format::Italic => RichText::new("I").italics(),
            Format::Heading => RichText::new("H"),
            Format::Bullet => RichText::new("•"),
            Format::Numbered => RichText::new("1."),
            Format::Quote => RichText::new("“”"),
            Format::Link => RichText::new("🔗"),
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
                "Link: type a Bible reference (Rom 5:8) or a web address between the parentheses"
            }
        }
    }
}

/// What the editor did this frame.
pub(super) struct NoteEditorOutput {
    pub(super) response: egui::Response,
    /// The pointer is on the toolbar: the field lost the keys to it only for a moment, so
    /// this is not the end of editing.
    pub(super) on_toolbar: bool,
}

/// The toolbar and the text field (`id`), `rows` high.
pub(super) fn note_editor(
    ui: &mut Ui,
    id: egui::Id,
    text: &mut String,
    rows: usize,
) -> NoteEditorOutput {
    let mut action = None;
    // A link is one line's text: a selection across lines cannot be one.
    let across_lines = selection_spans_lines(ui.ctx(), id, text);
    let toolbar = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for f in Format::TOOLBAR {
                let b = egui::Button::new(f.label())
                    .frame(false)
                    .min_size(egui::vec2(24.0, 20.0));
                let enabled = !(f == Format::Link && across_lines);
                if ui
                    .add_enabled(enabled, b)
                    .on_hover_text(f.hint())
                    .on_disabled_hover_text(LINK_ONE_LINE)
                    .clicked()
                {
                    action = Some(f);
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
    NoteEditorOutput {
        response,
        on_toolbar,
    }
}

/// Why the link button is off while the selection spans lines.
const LINK_ONE_LINE: &str = "Link: select text on one line (a link cannot run across lines)";

/// Whether the field's selection takes in a line break.
fn selection_spans_lines(ctx: &egui::Context, id: egui::Id, text: &str) -> bool {
    let Some(range) = egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range())
    else {
        return false;
    };
    let sel = range.as_sorted_char_range();
    text.chars()
        .skip(sel.start.0)
        .take(sel.end.0.saturating_sub(sel.start.0))
        .any(|c| c == '\n')
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

/// The list or quote marker a line starts with: (its byte length, the marker the next line
/// gets). `None` for any other line.
fn line_marker(line: &str) -> Option<(usize, String)> {
    let lead = line.len() - line.trim_start_matches(' ').len();
    let indent = &line[..lead];
    let body = &line[lead..];
    if lead == 0 && body.starts_with('>') {
        let len = if body.starts_with("> ") { 2 } else { 1 };
        return Some((len, "> ".into()));
    }
    for bullet in ["- ", "* ", "+ "] {
        if body.starts_with(bullet) {
            return Some((lead + 2, format!("{indent}{bullet}")));
        }
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits)
        && let Some(delim) = body[digits..]
            .chars()
            .next()
            .filter(|c| matches!(c, '.' | ')'))
        && body[digits + 1..].starts_with(' ')
        && let Ok(n) = body[..digits].parse::<u32>()
    {
        return Some((lead + digits + 2, format!("{indent}{}{delim} ", n + 1)));
    }
    None
}

/// What Enter does at char `at` of `text`, if it is in a list item or quote line past its
/// marker: the new text and cursor.
pub(super) fn list_continuation(text: &str, at: usize) -> Option<(String, Range<usize>)> {
    let b = byte_at(text, at);
    let start = text[..b].rfind('\n').map_or(0, |i| i + 1);
    let end = text[b..].find('\n').map_or(text.len(), |i| b + i);
    let line = &text[start..end];
    let (marker_len, next) = line_marker(line)?;
    if b < start + marker_len {
        return None;
    }
    if line[marker_len..].trim().is_empty() {
        // An empty item: Enter ends the list, leaving an empty line.
        let new = format!("{}{}", &text[..start], &text[end..]);
        let c = text[..start].chars().count();
        return Some((new, c..c));
    }
    let new = format!("{}\n{next}{}", &text[..b], &text[b..]);
    let c = at + 1 + next.chars().count();
    Some((new, c..c))
}

/// `text` with `f` applied to the selection `sel` (char indices), and the selection after.
pub(super) fn apply_format(text: &str, sel: Range<usize>, f: Format) -> (String, Range<usize>) {
    match f {
        Format::Bold => wrap(text, sel, "**"),
        Format::Italic => wrap(text, sel, "*"),
        Format::Link => link(text, sel),
        Format::Heading | Format::Bullet | Format::Numbered | Format::Quote => {
            line_format(text, sel, f)
        }
    }
}

/// Put `mark` around the selected part of each line the selection touches (past the line's
/// heading, list or quote marker, its outer spaces left outside: a Note's bold and italics end
/// with their line), or take it away when every part already has it. With nothing selected the
/// marks go in with the cursor between them.
fn wrap(text: &str, sel: Range<usize>, mark: &str) -> (String, Range<usize>) {
    let (a, b) = (byte_at(text, sel.start), byte_at(text, sel.end));
    let n = mark.len();
    let star = mark.as_bytes()[0];
    // Whether a run of `run` marks is the edge of a bold (two or more) or an italic (one or
    // three: the outer one of a bold italic).
    let opens = |run: usize| if n == 2 { run >= 2 } else { run % 2 == 1 };
    let run_before = |at: usize| {
        text.as_bytes()[..at]
            .iter()
            .rev()
            .take_while(|c| **c == star)
            .count()
    };
    let run_after = |at: usize| {
        text.as_bytes()[at..]
            .iter()
            .take_while(|c| **c == star)
            .count()
    };
    let has_mark =
        |p: &Range<usize>| p.end > p.start && opens(run_before(p.start)) && opens(run_after(p.end));
    let mut parts: Vec<Range<usize>> = Vec::new();
    if a < b {
        for line in obs_model::parse_note_text(text) {
            let (s, e) = (a.max(line.content), b.min(line.source.end));
            if s >= e {
                continue;
            }
            let part = &text[s..e];
            let (s, e) = (
                s + (part.len() - part.trim_start().len()),
                e - (part.len() - part.trim_end().len()),
            );
            if s >= e {
                continue;
            }
            // Marks selected with what they mark (as after the last press): the part is what
            // is inside them.
            let inner = s + if opens(run_after(s).min(e - s)) { n } else { 0 }
                ..e - if opens(run_before(e).min(e - s)) {
                    n
                } else {
                    0
                };
            parts.push(if inner != (s..e) && has_mark(&inner) {
                inner
            } else {
                s..e
            });
        }
    }
    if parts.is_empty() {
        parts.push(b..b);
    }
    let remove = parts.iter().all(has_mark);
    let mut new = String::with_capacity(text.len() + parts.len() * 2 * n);
    let mut at = 0;
    // Where the first and the last part are in `new`.
    let mut first = None;
    let mut last = 0;
    for p in &parts {
        // A part that already has the marks keeps them when others get them.
        let add = !remove && !has_mark(p);
        if remove {
            new.push_str(&text[at..p.start - n]);
            at = p.end + n;
        } else {
            new.push_str(&text[at..p.start]);
            at = p.end;
        }
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

/// `[selection](…)` with the cursor where the target goes; `[](…)` with it in the brackets.
/// A selection across lines is left as it is: a Note's link ends with its line.
fn link(text: &str, sel: Range<usize>) -> (String, Range<usize>) {
    let (a, b) = (byte_at(text, sel.start), byte_at(text, sel.end));
    if text[a..b].contains('\n') {
        return (text.to_string(), sel);
    }
    let new = format!("{}[{}]()", &text[..a], &text[a..b]);
    let new = format!("{new}{}", &text[b..]);
    let c = if a == b { sel.start + 1 } else { sel.end + 3 };
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

/// Heading, list or quote: on every line the selection touches, each read in its place in
/// the Note (a nested item is one under the item above it).
fn line_format(text: &str, sel: Range<usize>, f: Format) -> (String, Range<usize>) {
    let (a, b) = (byte_at(text, sel.start), byte_at(text, sel.end));
    let lines = obs_model::parse_note_text(text);
    let touched = touched_lines(text, a, b);
    let first = text[..touched[0].start].matches('\n').count();
    let chosen = &lines[first..first + touched.len()];
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
    let mut out: Vec<String> = Vec::new();
    for l in chosen {
        let line = &text[l.source.clone()];
        let level = l.kind.list_level();
        let lead = match level {
            Some(_) => &line[..line.len() - line.trim_start_matches([' ', '\t']).len()],
            None => "",
        };
        let rest = &text[l.content..l.source.end];
        // Text's leading spaces are its own; a marker goes before what it says.
        let rest = match l.kind {
            NoteLineKind::Text if !remove => rest.trim_start(),
            _ => rest,
        };
        let marker = match f {
            _ if remove => String::new(),
            Format::Heading => match l.kind {
                NoteLineKind::Heading(n) if n < 3 => "#".repeat(usize::from(n) + 1) + " ",
                NoteLineKind::Heading(_) => String::new(),
                _ => "# ".into(),
            },
            Format::Bullet => format!("{lead}- "),
            Format::Numbered => {
                let level = level.unwrap_or(0);
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
                format!("{lead}{n}. ")
            }
            _ => "> ".into(),
        };
        out.push(format!("{marker}{rest}"));
    }
    let replaced = out.join("\n");
    // Items nested under the last line when it stops being a list item: without an item above
    // they would read as text, so they move out, the first to the top level.
    let still_list = !remove && matches!(f, Format::Bullet | Format::Numbered);
    let after = first + touched.len();
    let mut moved: Vec<String> = Vec::new();
    if !still_list
        && let Some(depth) = lines
            .get(after)
            .and_then(|l| l.kind.list_level())
            .filter(|d| *d > 0)
    {
        for l in &lines[after..] {
            match l.kind.list_level() {
                Some(level) if level > 0 => {
                    let line = &text[l.source.clone()];
                    let indent = " ".repeat(2 * usize::from(level - level.min(depth)));
                    moved.push(format!("{indent}{}", line.trim_start_matches([' ', '\t'])));
                }
                _ => break,
            }
        }
    }
    let from = touched[0].start;
    let to = match moved.len() {
        0 => touched[touched.len() - 1].end,
        k => lines[after + k - 1].source.end,
    };
    let tail = moved.iter().map(|m| format!("\n{m}")).collect::<String>();
    let new = format!("{}{replaced}{tail}{}", &text[..from], &text[to..]);
    let start = text[..from].chars().count();
    (new, start..start + replaced.chars().count())
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
    fn bold_and_italic_mark_each_line_of_a_selection_past_its_marker() {
        // A Note's bold ends with its line, so each line gets its own marks.
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
        assert_eq!(new, "1. a\n  1. b\n  2. c\n2. d");
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
        // A line numbered under a numbered list follows on from its number.
        let (new, _) = applied("1. x\n   - y\nz", "z", Format::Numbered);
        assert_eq!(new, "1. x\n   - y\n2. z");
        // A parent that stops being an item: its nested items move out, not to text.
        let text = "- a\n  - b\n    - c\n  - d\n- e";
        let (new, _) = applied(text, "a", Format::Bullet);
        assert_eq!(new, "a\n- b\n  - c\n- d\n- e");
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
    fn link_puts_the_cursor_where_the_target_goes() {
        let (new, r) = apply_format("see this", chars("see this", "this"), Format::Link);
        assert_eq!(new, "see [this]()");
        assert_eq!(r, 11..11);
        let (new, r) = apply_format("x", 1..1, Format::Link);
        assert_eq!((new.as_str(), r), ("x[]()", 2..2));
        // Across lines there is no link to make: `[faith\nhope](…)` would show as typed.
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
    }
}
