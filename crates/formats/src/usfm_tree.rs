//! Turn `usfm3`'s concrete syntax tree into a USJ-shaped tree that keeps all text.
//!
//! `usfm3` 0.3's own USJ output drops whitespace-only text between character markers
//! (`\w a\w* \w b\w*` → `"a"`, `"b"`, words run together), so we read its CST, which keeps
//! byte spans for every text run, and rebuild the tree here:
//!
//! - nodes: `{type: book|para|char|note|chapter|verse|<other>, marker, content, attrs…}`;
//! - the one delimiter space after an opening marker is removed from its first text;
//! - a note's caller (`+`, `-`, a letter) is removed;
//! - `\w` attributes (`|strong="G26" lemma="…"`, or a bare default `|lemma`) become fields;
//! - verse numbers keep their source form (`3-4`, `5a`).

use serde_json::{Map, Value, json};

/// Parse `source` and return the tree.
pub(crate) fn parse(source: &str) -> Result<Value, String> {
    let doc = usfm3::parse(source, usfm3::Version::V3_1);
    let cst: Value = serde_json::from_str(&doc.to_cst_json()).map_err(|e| e.to_string())?;
    let mut content = Vec::new();
    for child in children(&cst) {
        convert(child, source, &mut content);
    }
    Ok(json!({ "type": "USJ", "content": content }))
}

fn children(node: &Value) -> &[Value] {
    node.get("children")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn slice<'a>(node: &Value, source: &'a str) -> &'a str {
    let start = node.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
    let end = node.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
    source.get(start..end).unwrap_or("")
}

fn str_field<'a>(node: &'a Value, name: &str) -> &'a str {
    node.get(name).and_then(Value::as_str).unwrap_or("")
}

/// Parse `|strong="G26" x-morph="V"` or a bare default attribute (`|grace` = lemma).
fn parse_attributes(raw: &str, out: &mut Map<String, Value>) {
    let body = raw.trim_start_matches('|').trim();
    if body.is_empty() {
        return;
    }
    if !body.contains('=') {
        out.insert("lemma".into(), Value::String(body.to_string()));
        return;
    }
    let mut rest = body;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(after) = after.strip_prefix('"') else {
            break;
        };
        let Some(close) = after.find('"') else {
            break;
        };
        out.insert(key, Value::String(after[..close].to_string()));
        rest = &after[close + 1..];
    }
}

fn convert(node: &Value, source: &str, out: &mut Vec<Value>) {
    match str_field(node, "type") {
        "text" => {
            let text = slice(node, source);
            if !text.is_empty() {
                out.push(Value::String(text.to_string()));
            }
        }
        "chapter" => {
            let number = node
                .get("number")
                .map(|n| n.to_string().trim_matches('"').to_string())
                .unwrap_or_default();
            out.push(json!({ "type": "chapter", "marker": "c", "number": number }));
        }
        "verse" => {
            let raw = slice(node, source);
            let number = raw
                .trim_start_matches('\\')
                .trim_start_matches('v')
                .trim()
                .to_string();
            out.push(json!({ "type": "verse", "marker": "v", "number": number }));
        }
        "marker" => {
            let mut marker = str_field(node, "marker").to_string();
            if marker.is_empty() {
                // Markers usfm3 does not know (`\\zz`) have no name; read it from the source.
                marker = slice(node, source)
                    .trim_start_matches('\\')
                    .trim_start_matches('+')
                    .trim()
                    .to_string();
            }
            let category = str_field(node, "category");
            let unknown_kind = if node.get("close").is_some() {
                "char"
            } else {
                "para"
            };
            let kind = match (category, marker.as_str()) {
                (_, "id") => "book",
                ("identification" | "paragraph", _) => "para",
                ("character", _) => "char",
                ("note", _) => "note",
                ("", _) => unknown_kind,
                (other, _) => other,
            };
            let mut map = Map::new();
            map.insert("type".into(), Value::String(kind.into()));
            map.insert("marker".into(), Value::String(marker));
            let mut content = Vec::new();
            let mut first_text = true;
            let mut in_caller = kind == "note";
            for child in children(node) {
                match str_field(child, "type") {
                    "attribute" => parse_attributes(slice(child, source), &mut map),
                    "text" => {
                        let mut text = slice(child, source);
                        if first_text {
                            // The delimiter after the marker is not content.
                            text = text
                                .strip_prefix([' ', '\n', '\t'])
                                .or_else(|| text.strip_prefix("\r\n"))
                                .unwrap_or(text);
                            first_text = false;
                        }
                        if in_caller {
                            // `\f + \fr …`: the caller is one token before the first marker.
                            let trimmed = text.trim_start();
                            let caller_len =
                                trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
                            text = trimmed[caller_len..].trim_start();
                            in_caller = false;
                        }
                        if !text.is_empty() {
                            content.push(Value::String(text.to_string()));
                        }
                    }
                    _ => {
                        first_text = false;
                        in_caller = false;
                        convert(child, source, &mut content);
                    }
                }
            }
            map.insert("content".into(), Value::Array(content));
            out.push(Value::Object(map));
        }
        _ => {
            // Unknown node kinds: keep their children so no text is lost silently.
            for child in children(node) {
                convert(child, source, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_spaces_between_words_and_drops_delimiters() {
        let tree = parse(
            "\\id GEN\n\\c 1\n\\p\n\\v 1 \\w In|strong=\"H1\"\\w* \\w the|H2\\w* \\nd \\+w LORD|strong=\"H3068\"\\+w*\\nd*\\f + \\fr 1:1 \\ft Or, x\\f* end.\n",
        )
        .unwrap();
        let para = tree["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "para")
            .unwrap();
        let c = para["content"].as_array().unwrap();
        assert_eq!(
            c[0],
            json!({ "type": "verse", "marker": "v", "number": "1" })
        );
        assert_eq!(c[2]["strong"], "H1");
        assert_eq!(c[2]["content"], json!(["In"]));
        assert_eq!(c[3], json!(" "), "space between two \\w markers is kept");
        assert_eq!(c[4]["lemma"], "H2");
        assert_eq!(c[6]["content"][0]["content"], json!(["LORD"]));
        let note = &c[7];
        assert_eq!(note["type"], "note");
        assert_eq!(
            note["content"][0]["content"],
            json!(["1:1 "]),
            "caller removed"
        );
    }

    #[test]
    fn attributes() {
        let mut m = Map::new();
        parse_attributes("|strong=\"G26\" x-morph=\"N-NSF\"", &mut m);
        assert_eq!(m["strong"], "G26");
        assert_eq!(m["x-morph"], "N-NSF");
        let mut m = Map::new();
        parse_attributes("|grace", &mut m);
        assert_eq!(m["lemma"], "grace");
    }
}
