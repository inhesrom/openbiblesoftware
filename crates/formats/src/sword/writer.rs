//! The target the SWORD markup parsers write into: one unit's text with its spans, Bible
//! verses and chapters, and note bodies, following the USFM importer's conventions.
//!
//! SWORD entries are fragments: a Bible verse may open a paragraph or a words-of-Jesus quote
//! that a later verse closes. A [`Writer`] therefore lives for a whole Bible book (or one
//! Commentary section or Lexicon entry) and pairs open and close tags by name across entries.
//! Paragraphs, headings and verse starts are opened lazily, at the first visible character, so
//! a heading that leads a verse's entry stays outside that verse.

use std::collections::{BTreeMap, HashSet};
use std::ops::Range;

use obs_model::{MorphemePart, NoteKind, ParaStyle, SpanKind, UnitId};
use obs_scripture::{
    Book, ListedVersification, Passage, PassageList, Standard, StrongsNumber, Verse, Versification,
    parse_reference,
};

use crate::text_buf::TextBuf;

/// What an opening tag stands for.
#[derive(Debug, Clone)]
pub(crate) enum Element {
    /// An inline span over the element's text.
    Span(SpanKind),
    /// A Scripture reference: its targets from the source, or `None` to parse its text.
    Reference(Option<PassageList>),
    Heading(u8),
    /// A Psalm superscription: the title slot (Verse 0) when it leads a Psalm.
    PsalmTitle {
        preverse: bool,
    },
    Para(ParaStyle),
    Note(NoteKind),
    /// Structure with nothing to keep (`div`, `lg`, a `w` without Strong's); not counted.
    Container,
    /// Markup with no place in the span vocabulary: its text is kept and it is counted.
    Dropped(String),
    /// Like [`Element::Dropped`], but its content is not text (figures, index entries).
    Hidden(String),
}

/// One finished unit.
#[derive(Debug, Default)]
pub(crate) struct UnitText {
    pub text: String,
    pub spans: Vec<(Range<usize>, SpanKind)>,
    pub verses: Vec<(Range<usize>, Verse, u16)>,
    pub chapters: u16,
}

impl UnitText {
    /// Open a Dictionary entry with its headword (see [`crate::text_buf::lead_with_headword`]).
    pub fn lead_with_headword(&mut self, headword: &str) {
        let shift = crate::text_buf::lead_with_headword(&mut self.text, &mut self.spans, headword);
        for (range, _, _) in &mut self.verses {
            *range = range.start + shift..range.end + shift;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Para(ParaStyle),
    Heading(u8),
    PsalmTitle,
}

#[derive(Debug, Clone, Copy)]
struct OpenVerse {
    verse: Verse,
    last: u16,
    /// Set at the verse's first visible character.
    start: Option<usize>,
}

#[derive(Debug)]
enum OpenKind {
    Span(SpanKind),
    Reference(Option<PassageList>),
    Heading,
    /// A Psalm title in the title slot; the verse to resume after it, if any.
    TitleSlot(Option<OpenVerse>),
    Para,
    Note,
    Container,
    Hidden,
}

#[derive(Debug)]
struct Open {
    name: String,
    kind: OpenKind,
    /// Where the element's text starts; set at its first visible character.
    start: Option<usize>,
    in_note: bool,
}

#[derive(Debug)]
struct NoteBody {
    kind: NoteKind,
    buf: TextBuf,
    targets: PassageList,
    tagged: Vec<(Range<usize>, Vec<MorphemePart>)>,
    /// Where its `note_ref` goes in the unit.
    at: usize,
}

/// See the module docs.
#[derive(Debug)]
pub(crate) struct Writer {
    buf: TextBuf,
    stack: Vec<Open>,
    milestones: BTreeMap<(String, String), Open>,
    /// Completed native Bible numbering. Non-Bible Resources use Standard.
    pub native: Option<ListedVersification>,
    /// Legacy KJV fragments often put an unnumbered title before Verse 1's text.
    pub unnumbered_psalm_titles: bool,
    hidden: usize,
    note: Option<NoteBody>,
    notes: Vec<(UnitId, TextBuf)>,
    note_count: u32,
    tagged: Vec<(Range<usize>, Vec<MorphemePart>)>,
    // Blocks.
    block: Option<(usize, BlockKind)>,
    styles: Vec<ParaStyle>,
    next_style: Option<ParaStyle>,
    heading: Option<BlockKind>,
    // Bible structure.
    book: Option<Book>,
    chapter: Option<u16>,
    chapter_start: Option<usize>,
    chapters: u16,
    /// In a chapter introduction (SWORD verse 0).
    intro: bool,
    verse: Option<OpenVerse>,
    verses: Vec<(Range<usize>, Verse, u16)>,
    seen: HashSet<Verse>,
    /// Context for relative references (`v. 5`).
    current: Option<Verse>,
    pub dropped: BTreeMap<String, u64>,
    pub warnings: Vec<String>,
}

impl Writer {
    pub fn new() -> Self {
        Self {
            buf: TextBuf::new(),
            stack: Vec::new(),
            milestones: BTreeMap::new(),
            native: None,
            unnumbered_psalm_titles: true,
            hidden: 0,
            note: None,
            notes: Vec::new(),
            note_count: 0,
            tagged: Vec::new(),
            block: None,
            styles: Vec::new(),
            next_style: None,
            heading: None,
            book: None,
            chapter: None,
            chapter_start: None,
            chapters: 0,
            intro: false,
            verse: None,
            verses: Vec::new(),
            seen: HashSet::new(),
            current: None,
            dropped: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }

    pub fn drop_markup(&mut self, name: &str) {
        *self.dropped.entry(name.to_string()).or_insert(0) += 1;
    }

    pub fn warn(&mut self, message: String) {
        self.warnings.push(message);
    }

    /// The Bible book being written, for verses and Psalm titles.
    pub fn set_book(&mut self, book: Option<Book>) {
        self.book = book;
    }

    /// The Verse relative references are read against.
    pub fn set_context(&mut self, verse: Option<Verse>) {
        self.current = verse;
    }

    pub fn parse_osis_ref(&self, text: &str) -> Option<PassageList> {
        parse_osis_ref_in(text, self.versification())
    }

    pub fn parse_reference(&self, text: &str) -> Option<PassageList> {
        parse_reference(text, self.versification(), self.current).ok()
    }

    fn versification(&self) -> &dyn Versification {
        self.native
            .as_ref()
            .map_or(&Standard as &dyn Versification, |v| v)
    }

    /// Milestones overlap ordinary XML containers and pair by identity, not nesting.
    pub fn open_milestone(&mut self, name: &str, id: &str, element: Element) {
        self.close_milestone(name, id);
        self.open(name, element);
        if let Some(open) = self.stack.pop() {
            self.milestones
                .insert((name.to_string(), id.to_string()), open);
        }
    }

    pub fn close_milestone(&mut self, name: &str, id: &str) {
        if let Some(open) = self.milestones.remove(&(name.to_string(), id.to_string())) {
            self.finish(open);
        }
    }

    // ---- Bible structure ------------------------------------------------------------------

    pub fn start_chapter(&mut self, chapter: u16) {
        self.close_chapter();
        self.chapter = Some(chapter);
        self.chapters += 1;
        self.intro = false;
    }

    /// Text that follows belongs to no verse (a book or chapter introduction).
    pub fn start_intro(&mut self) {
        self.close_verse();
        self.intro = true;
    }

    pub fn open_verse(&mut self, verse: Verse, last: u16) {
        self.close_verse();
        // Entries hold no space between verses; a block start swallows this one.
        self.buf.push_text(" ");
        self.intro = false;
        self.current = Some(verse);
        if !self.seen.insert(verse) {
            self.warn(format!(
                "{verse}: verse appears twice; its second text is kept outside any verse"
            ));
            return;
        }
        self.verse = Some(OpenVerse {
            verse,
            last,
            start: None,
        });
    }

    fn close_verse(&mut self) {
        if let Some(v) = self.verse.take() {
            let range = match v.start {
                Some(start) => start..self.buf.tracked_end.max(start),
                None => self.buf.pos()..self.buf.pos(),
            };
            self.verses.push((range, v.verse, v.last));
        }
    }

    fn close_chapter(&mut self) {
        self.close_verse();
        self.end_block();
        if let (Some(c), Some(start)) = (self.chapter, self.chapter_start.take()) {
            let end = self.buf.pos().max(start);
            self.buf
                .add_span(start..end, SpanKind::Chapter { chapter: c });
        }
    }

    // ---- Blocks ---------------------------------------------------------------------------

    /// End the current block; the next text starts a new one (with `style`, if given).
    pub fn para_break(&mut self, style: Option<ParaStyle>) {
        if self.note.is_some() {
            if let Some(n) = &mut self.note {
                n.buf.push_text(" ");
            }
            return;
        }
        self.end_block();
        self.next_style = style;
    }

    fn ensure_block(&mut self) {
        if self.block.is_some() {
            return;
        }
        let kind = self.heading.unwrap_or_else(|| {
            BlockKind::Para(
                self.next_style
                    .take()
                    .or_else(|| self.styles.last().copied())
                    .unwrap_or(ParaStyle::Prose),
            )
        });
        let start = self.buf.start_block();
        let title_slot = kind == BlockKind::PsalmTitle && self.verse.is_some();
        let starts_chapter = matches!(kind, BlockKind::Para(_)) || title_slot;
        if starts_chapter && self.chapter.is_some() && self.chapter_start.is_none() {
            self.chapter_start = Some(start);
        }
        self.block = Some((start, kind));
    }

    fn end_block(&mut self) {
        let Some((start, kind)) = self.block.take() else {
            return;
        };
        let end = self.buf.pos().max(start);
        match kind {
            BlockKind::Para(style) => self.buf.add_span(start..end, SpanKind::Para { style }),
            BlockKind::Heading(level) if end > start => {
                self.buf.add_span(start..end, SpanKind::Heading { level });
            }
            BlockKind::PsalmTitle if end > start => {
                self.buf.add_span(start..end, SpanKind::PsalmTitle);
            }
            _ => {}
        }
    }

    // ---- Text -----------------------------------------------------------------------------

    pub fn text(&mut self, s: &str) {
        if self.hidden > 0 || s.is_empty() {
            return;
        }
        let trimmed = s.trim_start();
        let lead = &s[..s.len() - trimmed.len()];
        let in_note = self.note.is_some();
        if trimmed.is_empty() {
            self.cur().push_text(s);
            return;
        }
        self.cur().push_text(lead);
        if !in_note {
            self.ensure_block();
            if self.heading.is_none() || self.verse.is_some_and(|v| v.verse.verse == 0) {
                self.ensure_verse_start();
            }
        }
        let buf = self.cur();
        buf.flush_space();
        let at = buf.pos();
        for open in self
            .stack
            .iter_mut()
            .chain(self.milestones.values_mut())
            .filter(|o| o.in_note == in_note)
        {
            if open.start.is_none() {
                open.start = Some(at);
            }
        }
        self.cur().push_text(trimmed);
    }

    fn ensure_verse_start(&mut self) {
        if let Some(v) = &mut self.verse
            && v.start.is_none()
        {
            self.buf.flush_space();
            let start = self.buf.pos();
            v.start = Some(start);
            self.buf.tracked_end = start;
        }
    }

    fn cur(&mut self) -> &mut TextBuf {
        match &mut self.note {
            Some(n) => &mut n.buf,
            None => &mut self.buf,
        }
    }

    /// Tag the word just written with a Strong's number (GBF `<WG25>`, ThML `<sync>`).
    pub fn strongs_after(&mut self, strongs: StrongsNumber) {
        let (buf, tagged) = match &mut self.note {
            Some(n) => (&n.buf, &mut n.tagged),
            None => (&self.buf, &mut self.tagged),
        };
        let Some(word) = last_word(&buf.text) else {
            self.warnings
                .push(format!("Strong's number {strongs} follows no word"));
            return;
        };
        let part = MorphemePart::new(strongs);
        match tagged.last_mut() {
            Some((range, parts)) if *range == word => parts.push(part),
            _ => tagged.push((word, vec![part])),
        }
    }

    /// Give the last tagged word's untagged morphemes a Morphology code.
    pub fn morph_after(&mut self, code: &str) {
        let tagged = match &mut self.note {
            Some(n) => &mut n.tagged,
            None => &mut self.tagged,
        };
        if let Some(part) = tagged
            .last_mut()
            .and_then(|(_, parts)| parts.iter_mut().find(|p| p.morph.is_none()))
        {
            part.morph = Some(code.to_string());
        }
    }

    // ---- Elements -------------------------------------------------------------------------

    pub fn open(&mut self, name: &str, element: Element) {
        let in_note = self.note.is_some();
        let kind = if self.hidden > 0 {
            OpenKind::Container
        } else {
            match element {
                Element::Span(kind) => OpenKind::Span(kind),
                Element::Reference(targets) => OpenKind::Reference(targets),
                Element::Dropped(label) => {
                    self.drop_markup(&label);
                    OpenKind::Container
                }
                Element::Hidden(label) => {
                    self.drop_markup(&label);
                    self.hidden += 1;
                    OpenKind::Hidden
                }
                Element::Container => OpenKind::Container,
                Element::Note(_)
                | Element::Para(_)
                | Element::Heading(_)
                | Element::PsalmTitle { .. }
                    if in_note =>
                {
                    OpenKind::Container
                }
                Element::Note(note_kind) => {
                    self.note = Some(NoteBody {
                        kind: note_kind,
                        buf: TextBuf::new(),
                        targets: PassageList::new(),
                        tagged: Vec::new(),
                        at: self.buf.pos(),
                    });
                    OpenKind::Note
                }
                Element::Para(style) => {
                    self.end_block();
                    self.styles.push(style);
                    OpenKind::Para
                }
                Element::Heading(_) | Element::PsalmTitle { .. } if self.heading.is_some() => {
                    OpenKind::Container
                }
                Element::Heading(level) => {
                    self.open_heading(BlockKind::Heading(level));
                    OpenKind::Heading
                }
                Element::PsalmTitle { preverse } => self.open_psalm_title(preverse),
            }
        };
        self.stack.push(Open {
            name: name.to_string(),
            kind,
            start: None,
            in_note,
        });
    }

    fn open_heading(&mut self, kind: BlockKind) {
        self.end_block();
        self.heading = Some(kind);
        self.buf.track = false;
    }

    /// A Psalm title leading its Psalm (in the chapter introduction, or before verse 1's
    /// text) fills the title slot, Verse 0; elsewhere it is a heading.
    fn open_psalm_title(&mut self, preverse: bool) -> OpenKind {
        let (Some(Book::Psalms), Some(chapter)) = (self.book, self.chapter) else {
            self.open_heading(BlockKind::PsalmTitle);
            return OpenKind::Heading;
        };
        if !self.intro && !preverse && !self.unnumbered_psalm_titles && self.verse.is_some() {
            // A numbered superscription is Scripture text in its native Verse, not Verse 0.
            self.open_heading(BlockKind::PsalmTitle);
            self.buf.track = true;
            self.ensure_block();
            self.ensure_verse_start();
            return OpenKind::Heading;
        }
        let title = Verse::new(Book::Psalms, chapter, 0);
        let leads = self.intro
            || (preverse || self.unnumbered_psalm_titles)
                && self.verse.is_some_and(|v| {
                    v.start.is_none() && v.verse == Verse::new(Book::Psalms, chapter, 1)
                });
        if !leads || self.seen.contains(&title) {
            self.open_heading(BlockKind::PsalmTitle);
            return OpenKind::Heading;
        }
        let resume = self.verse.take();
        self.end_block();
        self.seen.insert(title);
        self.verse = Some(OpenVerse {
            verse: title,
            last: 0,
            start: None,
        });
        self.heading = Some(BlockKind::PsalmTitle);
        OpenKind::TitleSlot(resume)
    }

    /// Close the innermost open element named `name` and any opened inside it. A close with
    /// no matching open is ignored.
    pub fn close(&mut self, name: &str) {
        let Some(i) = self.stack.iter().rposition(|o| o.name == name) else {
            return;
        };
        while self.stack.len() > i {
            if let Some(open) = self.stack.pop() {
                self.finish(open);
            }
        }
    }

    fn finish(&mut self, open: Open) {
        match open.kind {
            OpenKind::Span(kind) => {
                let buf = self.buf_for(open.in_note);
                if let Some(start) = open.start {
                    let end = buf.pos();
                    if end > start {
                        buf.add_span(start..end, kind);
                    }
                }
            }
            OpenKind::Reference(targets) => {
                self.finish_reference(open.start, targets, open.in_note)
            }
            OpenKind::Heading => {
                self.end_block();
                self.heading = None;
                self.buf.track = true;
            }
            OpenKind::TitleSlot(resume) => {
                self.end_block();
                self.heading = None;
                self.close_verse();
                self.verse = resume;
            }
            OpenKind::Para => {
                self.end_block();
                self.styles.pop();
            }
            OpenKind::Note => self.finish_note(),
            OpenKind::Container => {}
            OpenKind::Hidden => self.hidden = self.hidden.saturating_sub(1),
        }
    }

    fn buf_for(&mut self, in_note: bool) -> &mut TextBuf {
        match (&mut self.note, in_note) {
            (Some(n), true) => &mut n.buf,
            _ => &mut self.buf,
        }
    }

    fn finish_reference(
        &mut self,
        start: Option<usize>,
        targets: Option<PassageList>,
        in_note: bool,
    ) {
        let Some(start) = start else { return };
        let text = {
            let buf = self.buf_for(in_note);
            let end = buf.pos();
            if end <= start {
                return;
            }
            buf.text[start..end].to_string()
        };
        let targets = targets.or_else(|| self.parse_reference(&text).filter(|l| !l.is_empty()));
        let Some(targets) = targets else { return };
        let buf = self.buf_for(in_note);
        let end = buf.pos();
        buf.add_span(
            start..end,
            SpanKind::ScriptureRef {
                targets: targets.clone(),
            },
        );
        if in_note && let Some(note) = &mut self.note {
            for p in targets.iter() {
                note.targets.push(*p);
            }
        }
    }

    fn finish_note(&mut self) {
        let Some(mut note) = self.note.take() else {
            return;
        };
        if note.buf.text.is_empty() {
            return;
        }
        self.note_count += 1;
        let id = UnitId::note(self.note_count);
        for (range, parts) in note.tagged.drain(..) {
            note.buf.add_span(range, SpanKind::TaggedWord { parts });
        }
        self.buf.add_span(
            note.at..note.at,
            SpanKind::NoteRef {
                note: id.clone(),
                note_kind: note.kind,
            },
        );
        if note.kind == NoteKind::CrossReference && !note.targets.is_empty() {
            self.buf.add_span(
                note.at..note.at,
                SpanKind::Xref {
                    targets: note.targets,
                },
            );
        }
        self.notes.push((id, note.buf));
    }

    /// Close everything and hand over the unit; the Writer is ready for the next one.
    pub fn finish_unit(&mut self) -> UnitText {
        for (_, open) in std::mem::take(&mut self.milestones) {
            self.finish(open);
        }
        while let Some(open) = self.stack.pop() {
            self.finish(open);
        }
        self.close_chapter();
        self.end_block();
        for (range, parts) in self.tagged.drain(..) {
            self.buf.add_span(range, SpanKind::TaggedWord { parts });
        }
        let buf = std::mem::replace(&mut self.buf, TextBuf::new());
        let out = UnitText {
            text: buf.text,
            spans: buf.spans,
            verses: std::mem::take(&mut self.verses),
            chapters: self.chapters,
        };
        self.hidden = 0;
        self.styles.clear();
        self.next_style = None;
        self.heading = None;
        self.chapter = None;
        self.chapters = 0;
        self.intro = false;
        self.seen.clear();
        self.current = None;
        out
    }

    /// The note bodies written so far, as `note.<n>` units.
    pub fn take_notes(&mut self) -> Vec<(UnitId, TextBuf)> {
        std::mem::take(&mut self.notes)
    }
}

/// Byte range of the last word in `text` (letters and digits, with inner apostrophes),
/// skipping trailing punctuation and space.
fn last_word(text: &str) -> Option<Range<usize>> {
    let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’';
    let end = text
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
        .map(|(i, c)| i + c.len_utf8())?;
    let start = text[..end]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(end, |(i, _)| i);
    let start = start
        + text[start..end]
            .char_indices()
            .find(|(_, c)| c.is_alphanumeric())
            .map_or(0, |(i, _)| i);
    (end > start).then_some(start..end)
}

/// Parse an OSIS reference list (`Gen.1.1-Gen.1.3 John.3.16`, `Ps.23`, `KJV:Rom.5.8`) into
/// Standard Passages. `None` if any part is not in the 66-book canon.
#[cfg(test)]
fn parse_osis_ref(s: &str) -> Option<PassageList> {
    parse_osis_ref_in(s, &Standard)
}

fn parse_osis_ref_in(s: &str, v11n: &dyn Versification) -> Option<PassageList> {
    let mut list = PassageList::new();
    for part in s.split_whitespace() {
        let part = part.rsplit_once(':').map_or(part, |(_, r)| r);
        let (a, b) = part.split_once('-').unwrap_or((part, part));
        let start = osis_point(a, true, v11n)?;
        let end = osis_point(b, false, v11n)?;
        list.push(Passage::new(start, end).ok()?);
    }
    (!list.is_empty()).then_some(list)
}

fn osis_point(s: &str, first: bool, v11n: &dyn Versification) -> Option<Verse> {
    let s = s.split(['!', '@']).next().unwrap_or(s);
    let mut parts = s.split('.');
    let book = Book::from_osis(parts.next()?)?;
    let chapter: Option<u16> = parts.next().map(str::parse).transpose().ok()?;
    let verse: Option<u16> = parts.next().map(str::parse).transpose().ok()?;
    match (chapter, verse) {
        (Some(c), Some(v)) => {
            let verse = Verse::new(book, c, v);
            v11n.contains(verse).then_some(verse)
        }
        (Some(c), None) if first => v11n.first_verse(book, c),
        (Some(c), None) => v11n.last_verse_of(book, c),
        (None, _) => {
            let p = Passage::book(v11n, book)?;
            Some(if first { p.start() } else { p.end() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_word_skips_punctuation() {
        assert_eq!(last_word("In the beginning, "), Some(7..16));
        assert_eq!(last_word("don’t."), Some(0..7));
        assert_eq!(last_word("…"), None);
    }

    #[test]
    fn osis_refs() {
        let l = parse_osis_ref("Gen.1.1-Gen.1.3 KJV:John.3.16").unwrap();
        assert_eq!(l.to_string(), "Gen 1:1-3; John 3:16");
        assert_eq!(
            parse_osis_ref("Ps.23").unwrap().to_string(),
            "Ps 23:title-6"
        );
        assert!(parse_osis_ref("Tob.1.1").is_none());
    }

    #[test]
    fn spans_start_at_the_first_visible_character() {
        let mut w = Writer::new();
        w.set_book(Some(Book::John));
        w.start_chapter(3);
        w.open_verse(Verse::new(Book::John, 3, 16), 16);
        w.open("title", Element::Heading(1));
        w.text("God's love");
        w.close("title");
        w.text(" For ");
        w.open("q", Element::Span(SpanKind::Woj));
        w.text(" God so loved");
        w.open("note", Element::Note(NoteKind::Footnote));
        w.text("Or, ");
        w.open("hi", Element::Span(SpanKind::Italic));
        w.text("thus");
        // Closing `q` also closes the note and its `hi`, which were opened after it.
        w.close("q");
        let unit = w.finish_unit();
        assert_eq!(unit.text, "God's love\nFor God so loved");
        assert_eq!(
            unit.verses,
            vec![(11..27, Verse::new(Book::John, 3, 16), 16)]
        );
        assert!(unit.spans.contains(&(15..27, SpanKind::Woj)));
        assert!(
            unit.spans
                .contains(&(0..10, SpanKind::Heading { level: 1 }))
        );
        let notes = w.take_notes();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].1.text, "Or, thus");
    }
}
