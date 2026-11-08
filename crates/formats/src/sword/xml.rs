//! A tolerant tokenizer for the XML-like markup in SWORD entries (OSIS, ThML, TEI).
//!
//! Entries are fragments, not documents: a verse may open an element that a later verse
//! closes, so this only splits text from tags and never checks nesting. Anything that does not
//! look like a tag is text.

use std::borrow::Cow;

/// One piece of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    /// Text with entities decoded.
    Text(Cow<'a, str>),
    Start {
        name: &'a str,
        attrs: Vec<(&'a str, String)>,
        /// `<x/>`.
        empty: bool,
    },
    End(&'a str),
}

impl Token<'_> {
    /// An attribute of a start tag, entities decoded.
    pub fn attr(attrs: &[(&str, String)], name: &str) -> Option<String> {
        attrs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
    }
}

/// Split `s` into tokens.
pub(crate) fn tokenize(s: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            out.push(Token::Text(decode_entities(rest)));
            break;
        };
        if lt > 0 {
            out.push(Token::Text(decode_entities(&rest[..lt])));
        }
        let tail = &rest[lt..];
        if let Some(after) = tail.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        if let Some(after) = tail.strip_prefix("<![CDATA[") {
            let end = after.find("]]>").unwrap_or(after.len());
            out.push(Token::Text(Cow::Borrowed(&after[..end])));
            rest = if end == after.len() {
                ""
            } else {
                &after[end + 3..]
            };
            continue;
        }
        if tail.starts_with("<!") || tail.starts_with("<?") {
            rest = tail.find('>').map_or("", |end| &tail[end + 1..]);
            continue;
        }
        match tag(tail) {
            Some((token, used)) => {
                out.push(token);
                rest = &tail[used..];
            }
            None => {
                out.push(Token::Text(Cow::Borrowed("<")));
                rest = &tail[1..];
            }
        }
    }
    out
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '.')
}

/// Parse a tag at the start of `s` (which starts with `<`): the token and bytes used.
fn tag(s: &str) -> Option<(Token<'_>, usize)> {
    if let Some(end_tag) = s.strip_prefix("</") {
        let len = end_tag
            .find(|c: char| !is_name_char(c))
            .unwrap_or(end_tag.len());
        // Only space may follow the name; looking no further keeps a run of unclosed `</`
        // linear (#253).
        let close = len + (end_tag[len..].len() - end_tag[len..].trim_start().len());
        if len == 0 || !end_tag[close..].starts_with('>') {
            return None;
        }
        return Some((Token::End(&end_tag[..len]), 2 + close + 1));
    }
    if !s[1..].starts_with(|c: char| c.is_alphabetic()) {
        return None;
    }
    let mut i = 1;
    let name_len = s[i..]
        .find(|c: char| !is_name_char(c))
        .unwrap_or(s.len() - i);
    let name = &s[i..i + name_len];
    i += name_len;
    let mut attrs = Vec::new();
    let skip_ws = |i: usize| i + (s[i..].len() - s[i..].trim_start().len());
    loop {
        i = skip_ws(i);
        let rest = &s[i..];
        if rest.starts_with("/>") {
            let empty = true;
            return Some((Token::Start { name, attrs, empty }, i + 2));
        }
        if rest.starts_with('>') {
            let empty = false;
            return Some((Token::Start { name, attrs, empty }, i + 1));
        }
        let attr_len = rest.find(|c: char| !is_name_char(c)).unwrap_or(rest.len());
        if attr_len == 0 {
            return None;
        }
        let attr = &rest[..attr_len];
        i = skip_ws(i + attr_len);
        if !s[i..].starts_with('=') {
            // A bare attribute (`<td nowrap>`).
            attrs.push((attr, String::new()));
            continue;
        }
        i = skip_ws(i + 1);
        let v = &s[i..];
        let (value, used) = match v.chars().next() {
            Some(q @ ('"' | '\'')) => {
                // `<` cannot appear in a value; stopping there keeps unclosed quotes linear.
                let end = v[1..].find([q, '<'])?;
                if v[1 + end..].starts_with('<') {
                    return None;
                }
                (&v[1..1 + end], end + 2)
            }
            _ => {
                // Stopping at `<` too keeps a run of unclosed `<a b=x` linear (#253).
                let end = v
                    .find(|c: char| c.is_whitespace() || matches!(c, '>' | '/' | '<'))
                    .unwrap_or(v.len());
                (&v[..end], end)
            }
        };
        attrs.push((attr, decode_entities(value).into_owned()));
        i += used;
    }
}

/// Longest entity read, `&` to `;` inclusive: names and numbers stay under ten bytes.
const MAX_ENTITY_BYTES: usize = 11;

/// Decode XML and common HTML entities; unknown ones are kept as written.
///
/// The `;` is looked for only within [`MAX_ENTITY_BYTES`], so an `&` that starts no entity
/// costs constant work and the whole decode stays linear (#253).
pub(crate) fn decode_entities(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let reach = &tail.as_bytes()[..tail.len().min(MAX_ENTITY_BYTES)];
        let decoded = reach
            .iter()
            .position(|&b| b == b';')
            .and_then(|semi| entity(&tail[1..semi]).map(|c| (c, semi + 1)));
        match decoded {
            Some((c, used)) => {
                out.push(c);
                rest = &tail[used..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "hellip" => '…',
        "para" => '¶',
        "sect" => '§',
        "middot" => '·',
        "dagger" => '†',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start<'a>(name: &'a str, attrs: &[(&'a str, &str)], empty: bool) -> Token<'a> {
        Token::Start {
            name,
            attrs: attrs.iter().map(|(n, v)| (*n, v.to_string())).collect(),
            empty,
        }
    }

    #[test]
    fn tags_text_and_entities() {
        let t = tokenize(
            "<w lemma=\"strong:H07225\" morph='x'>In</w> &amp; <milestone type=x-p/><!-- c -->a < b",
        );
        assert_eq!(
            t,
            vec![
                start("w", &[("lemma", "strong:H07225"), ("morph", "x")], false),
                Token::Text("In".into()),
                Token::End("w"),
                Token::Text(" & ".into()),
                start("milestone", &[("type", "x-p")], true),
                Token::Text("a ".into()),
                Token::Text("<".into()),
                Token::Text(" b".into()),
            ]
        );
    }

    #[test]
    fn unterminated_tag_is_text() {
        let t = tokenize("x <note");
        assert_eq!(
            t,
            vec![
                Token::Text("x ".into()),
                Token::Text("<".into()),
                Token::Text("note".into()),
            ]
        );
    }

    /// AUD-FORMATS-7 (#253): an `&` with no `;` within entity reach costs constant work,
    /// so a megabyte of them decodes in linear time.
    #[test]
    fn unterminated_ampersands_decode_in_linear_time() {
        let s = "&".repeat(1 << 20);
        let started = std::time::Instant::now();
        assert_eq!(decode_entities(&s), s);
        let mixed = format!("{}&amp;", "& x".repeat(1 << 18));
        assert!(decode_entities(&mixed).ends_with("& x&"));
        let elapsed = started.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
    }

    /// #253: unclosed end tags and attribute quotes cost work up to the next `<` only, so
    /// a megabyte of them tokenizes in linear time and stays text.
    #[test]
    fn unclosed_tags_tokenize_in_linear_time() {
        let started = std::time::Instant::now();
        for unit in ["</a", "<a b=\"", "<a b='", "<a b=x"] {
            let s = unit.repeat((1 << 20) / unit.len());
            let text: String = tokenize(&s)
                .into_iter()
                .map(|t| match t {
                    Token::Text(t) => t.into_owned(),
                    other => panic!("{other:?}"),
                })
                .collect();
            assert_eq!(text, s);
        }
        let elapsed = started.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
    }

    #[test]
    fn entities() {
        assert_eq!(
            decode_entities("&#8220;a&#x2019;&rdquo;&bogus;"),
            "“a’”&bogus;"
        );
    }
}
