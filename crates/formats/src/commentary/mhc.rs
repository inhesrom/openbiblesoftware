use super::{
    StudyImport, html,
    references::{self, re},
};
use anyhow::{Context, Result, bail};
use obs_scripture::{Book, Passage, Standard, Verse, Versification};
use quick_xml::{Reader, events::Event};
use regex::{Captures, Regex};
use std::sync::OnceLock;

/// Read a decompressed MediaWiki export. Navigation pages and redirects are not text units.
pub fn import_mhc(import: &mut StudyImport, xml: &str) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    let mut title = String::new();
    let mut text = String::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) if e.name().as_ref() == b"title" => {
                title = quick_xml::escape::unescape(&reader.read_text(e.name())?.decode()?)?
                    .into_owned();
            }
            Event::Start(e) if e.name().as_ref() == b"text" => {
                text = quick_xml::escape::unescape(&reader.read_text(e.name())?.decode()?)?
                    .into_owned();
            }
            Event::End(e) if e.name().as_ref() == b"page" => {
                page(import, &title, &text).with_context(|| title.clone())?;
                title.clear();
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn page(import: &mut StudyImport, title: &str, source: &str) -> Result<()> {
    if source
        .trim_start()
        .to_ascii_uppercase()
        .starts_with("#REDIRECT")
    {
        return Ok(());
    }
    let segments: Vec<_> = title.split('/').collect();
    let Some(name) = segments.get(2) else {
        return Ok(());
    };
    if *name == "Preface" {
        return Ok(());
    }
    let normalized = name
        .replace("First ", "1 ")
        .replace("Second ", "2 ")
        .replace("Third ", "3 ");
    let book = Book::ALL
        .into_iter()
        .filter(|b| normalized.starts_with(b.name()))
        .max_by_key(|b| b.name().len())
        .or_else(|| {
            if *name == "Song of Solomon" {
                Some(Book::SongOfSongs)
            } else {
                None
            }
        })
        .with_context(|| format!("unrecognized MHC Bible book {name}"))?;
    let book_passage = Passage::book(&Standard, book).context("Bible book outside Standard")?;
    let subchapter = segments
        .last()
        .and_then(|s| s.strip_prefix("Chapter "))
        .and_then(|s| s.parse::<u16>().ok());
    let preface = segments.last() == Some(&"Preface");
    let source = templates(source, &mut import.report);
    static HEAD: OnceLock<Regex> = OnceLock::new();
    let heading = re(&HEAD, r"(?im)^\s*(=+)\s*(.*?)\s*=+\s*$");
    let mut chapter = subchapter;
    let mut key = subchapter
        .and_then(|c| Passage::chapter(&Standard, book, c))
        .map(|p| (p, ".chapter-intro"))
        .or_else(|| preface.then_some((book_passage, ".book-intro")));
    let mut start = 0;
    for cap in heading.captures_iter(&source) {
        let full = cap.get(0).context("missing heading match")?;
        let label = cap[2].trim();
        let lower = label.to_ascii_lowercase();
        let numbers: Vec<u16> = label
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|s| s.parse().ok())
            .collect();
        let next = if lower.starts_with("chap") && !numbers.is_empty() {
            chapter = Some(numbers[0]);
            Some((
                Passage::chapter(&Standard, book, numbers[0]).context("invalid MHC chapter")?,
                ".chapter-intro",
            ))
        } else if lower.starts_with("verse") && !numbers.is_empty() {
            let c = chapter.context("MHC verse section without chapter")?;
            let declared_end = *numbers.last().context("missing verse number")?;
            let last_verse = Standard
                .last_verse(book, c)
                .context("invalid MHC chapter")?;
            let end = declared_end.min(last_verse);
            if end != declared_end {
                import.report.warnings.push(format!("{} {c}:{}-{declared_end}: section heading end clipped to Standard verse {end}; prose unchanged",book.osis(),numbers[0]));
            }
            let refs = references::parse(
                &format!("{} {c}:{}-{end}", book.osis(), numbers[0]),
                None,
                false,
            )
            .with_context(|| {
                format!(
                    "invalid MHC section {} {c}:{}-{end}",
                    book.osis(),
                    numbers[0]
                )
            })?;
            Some((refs.passages()[0], ""))
        } else if lower.starts_with("preface") {
            Some((book_passage, ".book-intro"))
        } else {
            None
        };
        if let Some(next) = next {
            if let Some((passage, suffix)) = key {
                emit(import, passage, suffix, &source[start..full.start()])?;
            }
            key = Some(next);
            start = full.end();
        }
    }
    if let Some((passage, suffix)) = key {
        emit(import, passage, suffix, &source[start..])?;
    }
    Ok(())
}

fn templates(source: &str, report: &mut crate::ImportReport) -> String {
    let mut out = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let body = &rest[start + 2..];
        let mut depth = 1;
        let mut at = 0;
        while at + 1 < body.len() {
            if body[at..].starts_with("{{") {
                depth += 1;
                at += 2;
            } else if body[at..].starts_with("}}") {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                at += 2;
            } else {
                let Some(c) = body[at..].chars().next() else {
                    break;
                };
                at += c.len_utf8();
            }
        }
        if depth != 0 {
            out.push_str(&rest[start..]);
            return out;
        }
        let (name, value) = body[..at].split_once('|').unwrap_or((&body[..at], ""));
        match name.trim() {
            "sup" => out.push_str(&format!("<sup>{}</sup>", templates(value, report))),
            "small-caps" | "smaller" => {
                out.push_str(&format!("<small>{}</small>", templates(value, report)))
            }
            name => {
                *report
                    .dropped_markup
                    .entry(format!("template:{name}"))
                    .or_default() += 1;
            }
        }
        rest = &body[at + 2..];
    }
    out.push_str(rest);
    out
}

fn emit(import: &mut StudyImport, passage: Passage, suffix: &str, source: &str) -> Result<()> {
    static NAV: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    static BOLD: OnceLock<Regex> = OnceLock::new();
    static ITALIC: OnceLock<Regex> = OnceLock::new();
    static HEAD: OnceLock<Regex> = OnceLock::new();
    static PARA: OnceLock<Regex> = OnceLock::new();
    static NOTE: OnceLock<Regex> = OnceLock::new();
    static POEM: OnceLock<Regex> = OnceLock::new();
    let source = re(&NAV, r"(?m)^\s*\*\s*\[\[.*?\]\]\s*$").replace_all(source, "");
    let source = re(&LINK, r"\[\[([^\[\]]+)\]\]").replace_all(&source, |c: &Captures<'_>| {
        let (target, label) = c[1].split_once('|').unwrap_or((&c[1], &c[1]));
        if target.starts_with("Bible (King James)") {
            format!(
                "<a data-reference=\"{}\">{label}</a>",
                html_escape::encode_double_quoted_attribute(label)
            )
        } else {
            label.to_string()
        }
    });
    let source = re(&BOLD, r"(?s)'''(.*?)'''").replace_all(&source, "<b>$1</b>");
    let source = re(&ITALIC, r"(?s)''(.*?)''").replace_all(&source, "<i>$1</i>");
    let source = re(&HEAD, r"(?m)^\s*=+\s*(.*?)\s*=+\s*$").replace_all(&source, "<h4>$1</h4>");
    let source = re(&POEM, r"(?is)<poem>(.*?)</poem>").replace_all(&source, |c: &Captures<'_>| {
        format!(
            "<poem>{}</poem>",
            c[1].trim().lines().collect::<Vec<_>>().join("<br>")
        )
    });
    let source = re(&PARA, r"\n\s*\n|\n:+").replace_all(&source, "<br>");
    // Notes remain separate units, with stable section-local ids and zero-width markers.
    let notes = re(&NOTE, r"(?is)<ref(?:\s[^>]*)?>(.*?)</ref>");
    let mut text = crate::text_buf::TextBuf::new();
    let mut last = 0;
    let mut pending_space = false;
    for (index, cap) in notes.captures_iter(&source).enumerate() {
        let full = cap.get(0).context("missing note match")?;
        append(
            &mut text,
            html::render(
                &source[last..full.start()],
                Some(passage.start()),
                &mut import.report,
            ),
            pending_space || source[last..full.start()].starts_with(char::is_whitespace),
        );
        pending_space = source[last..full.start()].ends_with(char::is_whitespace);
        let id = obs_model::UnitId::new(format!(
            "{}-{}{suffix}.note.{}",
            passage.start().osis(),
            passage.end().osis(),
            index + 1
        ))?;
        let note = html::render(&cap[1], Some(passage.start()), &mut import.report);
        for (range, kind) in note.spans {
            import.builder.add_span(&id, range, kind);
        }
        import
            .builder
            .add_unit(id.clone(), obs_model::UnitKind::Note, None, note.text);
        text.add_span(
            text.pos()..text.pos(),
            obs_model::SpanKind::NoteRef {
                note: id,
                note_kind: obs_model::NoteKind::Footnote,
            },
        );
        import.report.notes += 1;
        last = full.end();
    }
    append(
        &mut text,
        html::render(
            &source[last..],
            Some(Verse::new(passage.start().book, passage.start().chapter, 1)),
            &mut import.report,
        ),
        pending_space || source[last..].starts_with(char::is_whitespace),
    );
    if text.text.contains("{{") {
        bail!("unclosed wiki template in {passage}");
    }
    import.section(passage, suffix, text);
    Ok(())
}

fn append(to: &mut crate::text_buf::TextBuf, from: crate::text_buf::TextBuf, separate: bool) {
    if separate
        && !to.text.is_empty()
        && !to.text.ends_with(char::is_whitespace)
        && !from.text.is_empty()
    {
        to.text.push(' ');
    }
    let start = to.text.len();
    to.text.push_str(&from.text);
    to.spans.extend(
        from.spans
            .into_iter()
            .map(|(r, k)| ((r.start + start)..(r.end + start), k)),
    );
}
