use super::{StudyImport, html, references::re};
use anyhow::{Context, Result, bail};
use obs_scripture::{Book, Passage, Standard, Verse, Versification};
use regex::Regex;
use std::{collections::BTreeSet, sync::OnceLock};

/// Import one Latin-1-decoded JFB01–JFB66 book file. Consecutive empty anchors extend
/// the preceding comment's Passage, rather than making empty units or copying its text.
pub fn import_jfb_book(import: &mut StudyImport, book: Book, source: &str) -> Result<()> {
    static ANCHOR: OnceLock<Regex> = OnceLock::new();
    static HR: OnceLock<Regex> = OnceLock::new();
    let anchors = re(
        &ANCHOR,
        r#"(?i)<a\s+name="(?:[1-3]?[a-z]+)(\d+)_(\d+)"\s*>\s*</a>"#,
    );
    let mut source = source.to_string();
    // These two verses have their own lemma but no anchor in the pinned edition (#39).
    for (b, chapter, verse, prefix) in [(Book::Mark, 4, 21, "Mr"), (Book::Zechariah, 1, 2, "Zec")] {
        if book == b
            && source.contains(&format!("NAME=\"{prefix}{chapter}_"))
            && !source.contains(&format!("NAME=\"{prefix}{chapter}_{verse}\""))
        {
            let needle = format!("<B>{verse}.");
            let chapter_start = source
                .find(&format!("NAME=\"{prefix}{chapter}_"))
                .context("missing chapter anchor")?;
            let chapter_end = source[chapter_start..]
                .find(&format!("<A NAME=\"Chapter{}\"", chapter + 1))
                .map_or(source.len(), |n| chapter_start + n);
            if let Some(at) = source[chapter_start..chapter_end].find(&needle) {
                source.insert_str(
                    chapter_start + at,
                    &format!("<A NAME=\"{prefix}{chapter}_{verse}\"> </A>"),
                );
            }
        }
    }
    let mut seen = BTreeSet::new();
    // A repeated anchor is navigation, not a second text unit. Keep all surrounding prose.
    source = anchors
        .replace_all(&source, |c: &regex::Captures<'_>| {
            if seen.insert((c[1].to_string(), c[2].to_string())) {
                c[0].to_string()
            } else {
                String::new()
            }
        })
        .into_owned();
    let marks: Vec<_> = anchors.captures_iter(&source).collect();
    if marks.is_empty() {
        bail!("{}: no JFB verse anchors", book.name());
    }
    let mut pending: Option<(Passage, crate::text_buf::TextBuf)> = None;
    let mut chapter_first: Option<Verse> = None;
    for (i, cap) in marks.iter().enumerate() {
        let verse = Verse::new(book, cap[1].parse()?, cap[2].parse()?);
        if chapter_first.is_none_or(|v| v.chapter != verse.chapter) {
            chapter_first = Some(verse);
        }
        if !Standard.contains(verse) {
            bail!("invalid JFB anchor {verse}");
        }
        let start = cap.get(0).context("anchor")?.end();
        let end = marks
            .get(i + 1)
            .and_then(|c| c.get(0))
            .map_or(source.len(), |m| m.start());
        let mut raw = &source[start..end];
        // Neither chapter navigation nor the closing navigation/banner is commentary.
        if let Some(at) = raw.find("<A NAME=\"Chapter") {
            raw = &raw[..at];
        }
        if let Some(hr) = re(&HR, r"(?i)<hr\s*>").find(raw) {
            raw = &raw[..hr.start()];
        }
        let text = html::render(raw, Some(verse), &mut import.report);
        if !text.text.trim().is_empty() {
            static LEMMA: OnceLock<Regex> = OnceLock::new();
            let first = re(&LEMMA, r"(?i)<b>\s*(\d+)[.,-]")
                .captures(raw)
                .and_then(|c| c[1].parse::<u16>().ok())
                .filter(|n| *n > 0 && *n <= verse.verse)
                .unwrap_or(verse.verse);
            let first = if pending
                .as_ref()
                .is_none_or(|(p, _)| p.start().chapter != verse.chapter)
            {
                chapter_first.context("missing chapter anchor")?.verse
            } else {
                first
            };
            let start = Verse::new(book, verse.chapter, first);
            if let Some((p, previous)) = &mut pending
                && start <= p.start()
            {
                if start.chapter != p.start().chapter {
                    bail!("out-of-order JFB section {start}");
                }
                let offset = previous.start_block();
                previous.text.push_str(&text.text);
                previous.spans.extend(
                    text.spans
                        .into_iter()
                        .map(|(r, k)| ((r.start + offset)..(r.end + offset), k)),
                );
                continue;
            }
            if let Some((p, text)) = pending.take() {
                let end = if p.start().chapter == start.chapter {
                    Verse::new(
                        book,
                        start.chapter,
                        start.verse.checked_sub(1).context("JFB section order")?,
                    )
                } else {
                    Standard
                        .last_verse_of(book, p.start().chapter)
                        .context("JFB chapter")?
                };
                import.section(Passage::new(p.start(), end)?, "", text);
            }
            pending = Some((Passage::verse(start), text));
        }
    }
    if let Some((p, text)) = pending {
        let end = Standard
            .last_verse_of(book, p.start().chapter)
            .context("JFB last chapter")?;
        import.section(Passage::new(p.start(), end)?, "", text);
    }
    Ok(())
}
