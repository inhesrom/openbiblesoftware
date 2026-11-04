//! Tolerant tokenizer for the simple HTML/TEI/ThML used by these editions. It never fetches
//! linked resources. Unknown markup keeps its text and is counted in the Import report.
use super::references::{self, re};
use crate::{ImportReport, text_buf::TextBuf};
use obs_model::{ParaStyle, SpanKind};
use obs_scripture::Verse;
use regex::Regex;
use std::sync::OnceLock;

pub(super) fn attr(tag: &str, name: &str) -> Option<String> {
    static ATTR: OnceLock<Regex> = OnceLock::new();
    re(
        &ATTR,
        r#"([\w:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#,
    )
    .captures_iter(tag)
    .find(|c| c[1].eq_ignore_ascii_case(name))
    .and_then(|c| {
        c.get(2)
            .or_else(|| c.get(3))
            .or_else(|| c.get(4))
            .map(|m| html_escape::decode_html_entities(m.as_str()).into_owned())
    })
}

pub(super) fn render(source: &str, context: Option<Verse>, report: &mut ImportReport) -> TextBuf {
    static TAG: OnceLock<Regex> = OnceLock::new();
    let mut out = TextBuf::new();
    let mut stack: Vec<(String, usize, SpanKind)> = Vec::new();
    let mut last = 0;
    for token in re(&TAG, r"(?s)<!--.*?-->|<[^>]*>").find_iter(source) {
        out.push_text(&html_escape::decode_html_entities(
            &source[last..token.start()],
        ));
        last = token.end();
        let raw = token.as_str();
        let tag = raw
            .trim_start_matches('<')
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if raw.starts_with("<!") {
            continue;
        }
        if raw.starts_with("</") {
            if let Some(i) = stack.iter().rposition(|(name, _, _)| name == &tag) {
                let (_, start, kind) = stack.remove(i);
                if out.pos() > start {
                    out.add_span(start..out.pos(), kind);
                }
            }
            if matches!(tag.as_str(), "p" | "div" | "h4" | "poem" | "li") {
                block(&mut out);
            }
            continue;
        }
        if matches!(tag.as_str(), "p" | "br" | "div" | "h4" | "poem" | "li") {
            block(&mut out);
        }
        let kind = match tag.as_str() {
            "i" | "em" => Some(SpanKind::Italic),
            "b" | "strong" => Some(SpanKind::Bold),
            "h4" | "title" => Some(SpanKind::Heading { level: 2 }),
            "hi" if attr(raw, "rend").as_deref() == Some("italic") => Some(SpanKind::Italic),
            "foreign" => Some(SpanKind::Foreign),
            "li" => Some(SpanKind::Para {
                style: ParaStyle::List,
            }),
            "p" => Some(SpanKind::Para {
                style: ParaStyle::Prose,
            }),
            "poem" => Some(SpanKind::Para {
                style: ParaStyle::Poetry1,
            }),
            "a" | "ref" | "scripref" => {
                let target = attr(raw, "data-reference")
                    .map(|s| (s, true))
                    .or_else(|| attr(raw, "passage").map(|s| (s, false)))
                    .or_else(|| attr(raw, "osisRef").map(|s| (s, false)))
                    .or_else(|| {
                        attr(raw, "href").and_then(|s| {
                            s.split_once("passage=")
                                .map(|(_, p)| (p.split('&').next().unwrap_or(p).to_string(), false))
                        })
                    });
                target.and_then(|(s,roman)| match references::parse(&s,context,roman) {
                    Some(targets) => Some(SpanKind::ScriptureRef { targets }),
                    None => {
                        let count = report.dropped_markup.entry("unparsed_scripture_ref".into()).or_default(); *count += 1;
                        if *count <= 10 { report.warnings.push(format!("Unparsed Bible reference {s:?} (context {context:?}); retained as plain text")); }
                        None
                    }
                })
            }
            "font" | "sup" | "small" => {
                *report.dropped_markup.entry(tag.clone()).or_default() += 1;
                None
            }
            "br" | "div" | "center" | "entryfree" | "term" | "lb" | "ol" => None,
            _ => {
                *report.dropped_markup.entry(tag.clone()).or_default() += 1;
                None
            }
        };
        if let Some(kind) = kind {
            out.flush_space();
            stack.push((tag, out.pos(), kind));
        }
    }
    out.push_text(&html_escape::decode_html_entities(&source[last..]));
    for (_, start, kind) in stack {
        if out.pos() > start {
            out.add_span(start..out.pos(), kind);
        }
    }
    references::annotate_prose(&mut out, context);
    // Wiki line breaks have no <p> wrapper. Keep paragraph boundaries for layout/Search.
    let mut start = 0;
    for line in out.text.split_inclusive('\n') {
        let end = start + line.trim_end_matches('\n').len();
        if end > start
            && !out.spans.iter().any(|(r, k)| {
                matches!(k, SpanKind::Para { .. }) && r.start <= start && r.end >= end
            })
        {
            out.spans.push((
                start..end,
                SpanKind::Para {
                    style: ParaStyle::Prose,
                },
            ));
        }
        start += line.len();
    }
    out
}

fn block(out: &mut TextBuf) {
    if !out.text.is_empty() && !out.text.ends_with('\n') {
        out.start_block();
    }
}
