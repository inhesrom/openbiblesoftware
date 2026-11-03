//! STEPBible TBESH / TBESG: "Translators Brief lexicon of Extended Strongs" (TSV).
//!
//! Each data row is `eStrong  dStrong[ = relation]  uStrong  lemma  transliteration  morph
//! gloss  meaning`. One unit per row, keyed by its dStrong number (`H1254A`, `H0001G`) and its
//! eStrong number, with the base number as a secondary Key and each comma-separated lemma
//! form as a Lemma Key.
//!
//! The entry reads: lemma (bold) and transliteration; the gloss; the relation to another
//! number ("In Aramaic of H1G", linked); then the meaning, whose HTML-like markup maps as
//! `<b>`/`<def>` → `bold`, `<i>`/`<note>` → `italic`, `<ref='Mat.6.24'>` → `scripture_ref`,
//! `<br>`/`<BR />`/`<lb />` → a new block; Strong's numbers written in it are linked. The morph
//! column is not shown (STEPBible's brief codes are not plain English).

use obs_model::SpanKind;
use obs_scripture::StrongsNumber;

use super::{Entry, LexiconError, LexiconOptions, LexiconWriter, strongs_word};
use crate::report::ImportReport;
use obs_model::Resource;

/// Column positions in a TBES row.
const LEMMA: usize = 3;
const TRANSLITERATION: usize = 4;
const MORPH: usize = 5;
const GLOSS: usize = 6;
const MEANING: usize = 7;

/// Convert a TBESH or TBESG file. `skip_columns` names columns (by their header) to leave out
/// of the entry text, e.g. `Meaning`; only the transliteration, morph, gloss and meaning
/// columns may be skipped.
pub fn import_tbes(
    o: &LexiconOptions,
    text: &str,
    skip_columns: &[String],
) -> Result<(Resource, ImportReport), LexiconError> {
    let file = if o.source_file.is_empty() {
        "TBES".to_string()
    } else {
        o.source_file.clone()
    };
    let lines: Vec<&str> = text.lines().collect();
    let header_at = lines
        .iter()
        .position(|l| {
            let mut f = l.split('\t');
            f.next().is_some_and(|c| c.trim().starts_with("eStrong"))
                && f.next().is_some_and(|c| c.trim() == "dStrong")
        })
        .ok_or_else(|| LexiconError::Parse {
            file: file.clone(),
            message: "no `eStrong  dStrong …` header row".into(),
        })?;
    let header: Vec<&str> = lines[header_at].split('\t').map(str::trim).collect();
    let mut skip = [false; 8];
    for name in skip_columns {
        match header.iter().position(|h| h == name) {
            Some(i @ (TRANSLITERATION | MORPH | GLOSS | MEANING)) => skip[i] = true,
            _ => return Err(LexiconError::UnknownColumn(name.clone())),
        }
    }
    let mut w = LexiconWriter::new(o, "stepbible-tbes", vec![file])?;
    for (n, line) in lines.iter().enumerate().skip(header_at + 1) {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        // Separator lines (`=====`) have no columns.
        if fields.len() < 2 {
            continue;
        }
        let estrong = fields.first().and_then(|f| strongs_word(f.trim()));
        match (estrong, fields.len() > GLOSS) {
            (Some(e), true) => {
                let entry = row(e, &fields, &skip);
                w.add(entry);
            }
            _ => w.warn(format!("line {}: not an entry row", n + 1)),
        }
    }
    w.finish()
}

fn field<'a>(fields: &[&'a str], i: usize, skip: &[bool; 8]) -> &'a str {
    if skip.get(i).copied().unwrap_or(false) {
        return "";
    }
    fields.get(i).map_or("", |f| f.trim())
}

fn row(estrong: StrongsNumber, fields: &[&str], skip: &[bool; 8]) -> Entry {
    // `H0001H = a Part of` → dStrong H1H, relation "a Part of".
    let dfield = fields.get(1).copied().unwrap_or_default();
    let (dnum, relation) = dfield.split_once('=').unwrap_or((dfield, ""));
    let (dnum, dstrong) = match strongs_word(dnum.trim()) {
        Some(d) => (dnum.trim(), d),
        None => (fields.first().map_or("", |f| f.trim()), estrong),
    };
    let relation = relation.trim();
    let ustrong: Vec<StrongsNumber> = fields
        .get(2)
        .copied()
        .unwrap_or_default()
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(strongs_word)
        .collect();
    let lemma = field(fields, LEMMA, skip);
    // STEPBible tells some individuals apart by letter case alone (H2148V, H2148v), which a
    // Strong's number does not keep, so the unit id keeps the letter as written.
    let letter: String = dnum
        .chars()
        .rev()
        .take_while(char::is_ascii_alphabetic)
        .collect();
    let id = match dstrong.homograph() {
        Some(_) => format!(
            "{}{}{letter}",
            dstrong.language().prefix(),
            dstrong.number()
        ),
        None => dstrong.to_string(),
    };
    let mut e = Entry::new(id, lemma);
    e.key_strongs(dstrong);
    e.key_strongs(estrong);
    for form in lemma.split(',') {
        e.key_lemma(form);
    }

    e.styled(SpanKind::Bold, lemma);
    let translit = field(fields, TRANSLITERATION, skip);
    if !translit.is_empty() {
        e.text(" ");
        e.styled(SpanKind::Italic, translit);
    }
    e.block();
    e.text(field(fields, GLOSS, skip));
    e.block();
    let others: Vec<StrongsNumber> = ustrong.into_iter().filter(|u| *u != dstrong).collect();
    if !relation.is_empty() && !others.is_empty() {
        let mut chars = relation.chars();
        let capital: String = chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect())
            .unwrap_or_default();
        e.text(&capital);
        for (i, n) in others.iter().enumerate() {
            e.text(if i == 0 { " " } else { ", " });
            e.strongs_number(*n);
        }
        e.text(".");
        e.block();
    }
    let meaning = field(fields, MEANING, skip);
    if !meaning.is_empty() {
        markup(&mut e, meaning);
    }
    e
}

/// What an open tag in the meaning column becomes.
enum Open {
    Span(SpanKind),
    Ref(String),
    Plain,
}

/// Render the meaning column's HTML-like markup into `e`.
fn markup(e: &mut Entry, html: &str) {
    let mut stack: Vec<(String, usize, Open)> = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            text(e, rest);
            break;
        };
        text(e, &rest[..lt]);
        let after = &rest[lt..];
        let Some(gt) = after.find('>') else {
            text(e, after);
            break;
        };
        let tag = &after[1..gt];
        rest = &after[gt + 1..];
        let closing = tag.starts_with('/');
        let body = tag.trim_start_matches('/').trim_end_matches('/').trim();
        let name_end = body
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(body.len());
        let name = body[..name_end].to_ascii_lowercase();
        let attrs = &body[name_end..];
        if matches!(name.as_str(), "br" | "lb") {
            e.block();
            continue;
        }
        if closing {
            if let Some(i) = stack.iter().rposition(|(n, _, _)| *n == name) {
                while stack.len() > i {
                    if let Some((_, start, open)) = stack.pop() {
                        close(e, start, open);
                    }
                }
            }
            continue;
        }
        let open = match name.as_str() {
            "b" | "def" => Open::Span(SpanKind::Bold),
            "i" | "note" => Open::Span(SpanKind::Italic),
            "ref" => Open::Ref(quoted(attrs)),
            "re" | "author" | "date" | "a" | "corr" | "greek" => Open::Plain,
            other => {
                e.drop_markup(&format!("<{other}>"));
                Open::Plain
            }
        };
        if tag.ends_with('/') {
            continue;
        }
        let start = e.mark();
        stack.push((name, start, open));
    }
    while let Some((_, start, open)) = stack.pop() {
        close(e, start, open);
    }
    e.block();
}

fn close(e: &mut Entry, start: usize, open: Open) {
    match open {
        Open::Span(kind) => e.span(start, kind),
        Open::Ref(osis) => e.scripture_ref(start, &osis),
        Open::Plain => {}
    }
}

/// The first quoted value in `attrs` (`='Mat.6.24'` → `Mat.6.24`).
fn quoted(attrs: &str) -> String {
    let Some(q) = attrs.find(['\'', '"']) else {
        return String::new();
    };
    let quote = attrs[q..].chars().next().unwrap_or('\'');
    let value = &attrs[q + 1..];
    value[..value.find(quote).unwrap_or(value.len())].to_string()
}

/// Plain text from the meaning column: entities decoded, `__1.` sense markers unwrapped.
fn text(e: &mut Entry, s: &str) {
    let decoded = s
        .replace("__", "")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    e.text_linking_strongs(&decoded);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(rows: &str, skip: &[&str]) -> Resource {
        let text = format!(
            "TBESX test\n\neStrong#\tdStrong\tuStrong\tHebrew\tTransliteration\tMorph\tGloss\tMeaning\n\n{rows}"
        );
        let skip: Vec<String> = skip.iter().map(|s| s.to_string()).collect();
        import_tbes(
            &LexiconOptions {
                resource_id: "tbes".into(),
                ..LexiconOptions::default()
            },
            &text,
            &skip,
        )
        .unwrap()
        .0
    }

    #[test]
    fn meaning_markup_maps_to_spans() {
        let r = convert(
            "G1410\tG1410 =\tG1410\tδύναμαι\tdunamai\tG:V\tbe able\t <b>δύναμαι</b>, <BR /><i>to be able</i>: <ref='Mat.6.24'>Mat.6:24</ref>, <ref='Sir.1.1'>Si 1:1</ref> (cf. H2438H) &gt; <x>odd</x>\n",
            &[],
        );
        let u = &r.units[0];
        assert_eq!(
            u.text,
            "δύναμαι dunamai\nbe able\nδύναμαι,\nto be able: Mat.6:24, Si 1:1 (cf. H2438H) > odd"
        );
        let names: Vec<(&str, &str)> = u
            .spans
            .iter()
            .filter(|s| s.kind.name() != "para")
            .map(|s| (s.kind.name(), &u.text[s.range()]))
            .collect();
        assert_eq!(
            names,
            [
                ("bold", "δύναμαι"),
                ("italic", "dunamai"),
                ("bold", "δύναμαι"),
                ("italic", "to be able"),
                ("scripture_ref", "Mat.6:24"),
                ("tagged_word", "H2438H"),
            ]
        );
        assert_eq!(r.dropped_markup.get("<x>"), Some(&1));
        assert_eq!(
            r.dropped_markup
                .get("ref outside the Standard versification"),
            Some(&1)
        );
    }

    #[test]
    fn rows_key_by_dstrong_estrong_and_base_with_a_linked_relation() {
        let r = convert(
            "H0002\tH0002 = in Aramaic of\tH0001G\tאַב\tav\tA:N-M\tfather\tfather<BR>secret\n",
            &["Meaning"],
        );
        let u = &r.units[0];
        assert_eq!(u.id.as_str(), "H2");
        assert_eq!(u.text, "אַב av\nfather\nIn Aramaic of H1G.");
        assert!(u.spans.iter().any(|s| matches!(
            &s.kind,
            SpanKind::TaggedWord { parts } if parts[0].strongs.to_string() == "H1G"
        )));
        let keys: Vec<String> = r.keys.iter().map(|k| k.key.to_string()).collect();
        assert_eq!(keys, ["H2", "אַב"]);
    }

    #[test]
    fn unit_ids_keep_the_case_of_the_dstrong_letter() {
        let r = convert(
            "H2148a\tH2148V =\tH2148V\tזְכַרְיָהוּ\tze.khar.yah\tN:N-M-P\tZechariah\t\nH2148a\tH2148v =\tH2148v\tזְכַרְיָה\tze.khar.yah\tN:N-M-P\tZechariah\t\n",
            &[],
        );
        let ids: Vec<&str> = r.units.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, ["H2148V", "H2148v"]);
        let h2148v = r
            .keys
            .iter()
            .filter(|k| k.key.to_string() == "H2148V")
            .count();
        assert_eq!(h2148v, 2);
    }

    #[test]
    fn only_display_columns_may_be_skipped() {
        let err = import_tbes(
            &LexiconOptions {
                resource_id: "tbes".into(),
                ..LexiconOptions::default()
            },
            "eStrong#\tdStrong\tuStrong\tHebrew\tTransliteration\tMorph\tGloss\tMeaning\n",
            &["Hebrew".to_string()],
        )
        .unwrap_err();
        assert!(matches!(err, LexiconError::UnknownColumn(c) if c == "Hebrew"));
    }
}
