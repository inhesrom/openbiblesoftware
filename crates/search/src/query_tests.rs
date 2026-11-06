#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::morph::MorphCriteria;
use crate::query::*;

/// A Lexicon with two transliterations.
fn lexicon(t: &str) -> Vec<String> {
    match t {
        "agape" => vec!["ἀγάπη".into()],
        "bara" => vec!["בָּרָא".into(), "בָּרָה".into()],
        _ => vec![],
    }
}

/// A Search index's Morphology codes and their tokens.
struct Codes(&'static [(&'static str, &'static str)]);

impl MorphVocabulary for Codes {
    fn tokens(&self, criteria: &MorphCriteria) -> Vec<String> {
        self.0
            .iter()
            .filter(|(_, code)| criteria.matches_code(code))
            .map(|(t, _)| (*t).to_string())
            .collect()
    }
}

const CODES: Codes = Codes(&[
    ("morph1", "V-AAM-2P"),
    ("morph2", "V-AAI-3S"),
    ("morph3", "N-GSF"),
    ("morph4", "V-2AAM-2S"),
    ("morph5", "HVqp3ms"),
]);

fn expanded(c: Compiled) -> String {
    match c {
        Compiled::Match(s) => s,
        Compiled::Morph(m) => expanded(m.expand(&CODES)),
        Compiled::Nothing => "<nothing>".into(),
    }
}

fn fts(q: &str) -> String {
    expanded(to_fts5(q, &lexicon).unwrap_or_else(|e| panic!("{q}: {e}")))
}

#[test]
fn every_form_in_the_search_syntax() {
    let table: &[(&str, &str)] = &[
        // All words.
        ("love neighbour", r#"text : "love" AND text : "neighbour""#),
        // Normalized: case and diacritics fold; ς → σ.
        ("LOVE Naïve", r#"text : "love" AND text : "naive""#),
        // Phrase.
        (r#""love thy neighbour""#, r#"text : "love thy neighbour""#),
        // Boolean.
        ("faith OR hope", r#"text : "faith" OR text : "hope""#),
        ("faith -works", r#"text : "faith" NOT text : "works""#),
        ("faith NOT works", r#"text : "faith" NOT text : "works""#),
        (
            "(faith OR hope) love",
            r#"(text : "faith" OR text : "hope") AND text : "love""#,
        ),
        ("faith AND hope", r#"text : "faith" AND text : "hope""#),
        (
            "grace peace -law -works",
            r#"(text : "grace" AND text : "peace") NOT text : "law" NOT text : "works""#,
        ),
        // Lowercase operators are words.
        (
            "faith or hope",
            r#"text : "faith" AND text : "or" AND text : "hope""#,
        ),
        // Prefix.
        ("lov*", r#"text : "lov" *"#),
        (
            r#"lov* "thy neigh"*"#,
            r#"text : "lov" * AND text : "thy neigh" *"#,
        ),
        // Strong's numbers: bare or with strong:, extended or base.
        ("strong:G26", r#"strong : "g26""#),
        ("G26", r#"strong : "g26""#),
        ("H1254A", r#"strong : "h1254a""#),
        ("strong:h1254a", r#"strong : "h1254a""#),
        ("strong:G05485", r#"strong : "g5485""#),
        // Lemma: original script, or transliteration through the Lexicon.
        ("lemma:ἀγάπη", r#"lemma : "αγαπη""#),
        ("lemma:agape", r#"lemma : "αγαπη""#),
        ("lemma:bara", r#"lemma : "ברא" OR lemma : "ברה""#),
        (r#"lemma:"ἀγάπη""#, r#"lemma : "αγαπη""#),
        // Combined: the Word Study "Translated as" filter.
        (
            r#"strong:G5485 "favour""#,
            r#"strong : "g5485" AND text : "favour""#,
        ),
        // A word the normalizer splits becomes a phrase.
        ("first-born", r#"text : "first born""#),
        ("3:16", r#"text : "3 16""#),
        // Lenient: unclosed quotes and parentheses close, strays and dangling operators go.
        (r#""love thy"#, r#"text : "love thy""#),
        ("(faith OR hope", r#"text : "faith" OR text : "hope""#),
        ("faith) hope", r#"text : "faith" AND text : "hope""#),
        ("faith OR", r#"text : "faith""#),
        ("OR faith", r#"text : "faith""#),
        ("faith NOT", r#"text : "faith""#),
        ("faith - hope", r#"text : "faith" AND text : "hope""#),
        ("--faith", r#"text : "faith""#),
        ("() faith", r#"text : "faith""#),
        // Nested groups keep their meaning.
        (
            "(a OR (b c)) -(d OR e)",
            r#"(text : "a" OR (text : "b" AND text : "c")) NOT (text : "d" OR text : "e")"#,
        ),
        // Text that looks like FTS5 syntax is only ever a quoted term.
        (
            r#"NEAR(a b) ^c {d}"#,
            r#"text : "near" AND text : "a" AND text : "b" AND text : "c" AND text : "d""#,
        ),
    ];
    for (q, want) in table {
        assert_eq!(fts(q), *want, "query {q:?}");
    }
    // An unknown transliteration matches nothing; ANDed, so does the whole query.
    assert_eq!(fts("lemma:zzz"), "<nothing>");
    assert_eq!(fts("lemma:zzz love"), "<nothing>");
    assert_eq!(fts("lemma:zzz OR love"), r#"text : "love""#);
    assert_eq!(fts("love -lemma:zzz"), r#"text : "love""#);
    // Bare-word Strong's recognition needs the uppercase letter: h2o is a word.
    assert_eq!(fts("h2o"), r#"text : "h2o""#);
}

#[test]
fn errors_are_positioned() {
    let err = |q: &str| to_fts5(q, &NoLexicon).unwrap_err();
    let e = err("strong:X12 love");
    assert!(matches!(e.kind, QueryErrorKind::BadStrongs(_)));
    assert_eq!(e.span, 7..10);
    let e = err("love foo:bar");
    assert_eq!(e.kind, QueryErrorKind::UnknownField("foo".into()));
    assert_eq!(e.span, 5..9);
    let e = err("love strong:");
    assert_eq!(e.kind, QueryErrorKind::EmptyField("strong"));
    let e = err("-works");
    assert_eq!(e.kind, QueryErrorKind::OnlyExcluded);
    assert_eq!(e.span, 0..6);
    let e = err("faith OR -works");
    assert_eq!(e.kind, QueryErrorKind::ExcludedAlternative);
    assert_eq!(e.span, 9..15);
    for empty in ["", "   ", "()", "\"\"", "OR", "* - ,"] {
        assert_eq!(err(empty).kind, QueryErrorKind::Empty, "{empty:?}");
    }
    assert!(
        err("strong:X")
            .to_string()
            .contains("not a Strong's number")
    );
}

#[test]
fn parse_builds_the_ast() {
    let q = parse("(faith OR hope) -works").unwrap();
    let Query::And(parts) = q else { panic!() };
    assert!(matches!(&parts[0], Query::Or(a) if a.len() == 2));
    assert!(matches!(&parts[1], Query::Not { span, .. } if *span == (16..22)));
    let q = parse("H1254A").unwrap();
    assert!(matches!(q, Query::Strong { number, .. } if number.is_extended()));
}

/// Check that an FTS5 expression contains only quoted strings and the syntax we emit:
/// `text`/`strong`/`lemma` column filters, `:`, `*`, `(`, `)`, `AND`, `OR`, `NOT`.
pub(crate) fn only_safe_syntax(expr: &str) -> Result<(), String> {
    let mut chars = expr.chars().peekable();
    let mut depth = 0i32;
    while let Some(c) = chars.next() {
        match c {
            ' ' | ':' | '*' => {}
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err("unbalanced )".into());
                }
            }
            '"' => loop {
                match chars.next() {
                    None => return Err("unterminated string".into()),
                    Some('"') if chars.peek() == Some(&'"') => {
                        chars.next();
                    }
                    Some('"') => break,
                    Some(_) => {}
                }
            },
            c if c.is_ascii_alphabetic() => {
                let mut word = String::from(c);
                while let Some(&n) = chars.peek() {
                    if !n.is_ascii_alphabetic() {
                        break;
                    }
                    word.push(n);
                    chars.next();
                }
                if !["AND", "OR", "NOT", "text", "strong", "lemma", "morph"]
                    .contains(&word.as_str())
                {
                    return Err(format!("bare word {word:?}"));
                }
            }
            c => return Err(format!("unexpected {c:?}")),
        }
    }
    if depth != 0 {
        return Err("unbalanced (".into());
    }
    Ok(())
}

/// Random queries from pieces that are syntax in FTS5 or in our grammar.
pub(crate) fn random_queries(n: usize, seed: u64) -> Vec<String> {
    const PIECES: &[&str] = &[
        "love",
        "LORD",
        " ",
        " ",
        " ",
        "\"",
        "(",
        ")",
        "*",
        "-",
        ":",
        "^",
        "+",
        "{",
        "}",
        ",",
        ".",
        "'",
        "’",
        "OR",
        "AND",
        "NOT",
        "NEAR",
        "NEAR(",
        "or",
        "strong:",
        "lemma:",
        "text:",
        "foo:",
        "morph:",
        "@",
        "@verb",
        "@aorist",
        "aorist",
        "imperative",
        "consecutive",
        "imperfect",
        "@xyz",
        "G26",
        "H1254A",
        "g",
        "h",
        "5485",
        "ἀγάπη",
        "בָּרָא",
        "naïve",
        "\\",
        "\0",
        "\"\"",
        "x*y",
        "--",
        "a-b",
        ")(",
        "\t",
        "\n",
        "λόγος",
        "ﬁ",
        "🙂",
        "%",
        "_",
        "@",
        "$",
    ];
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    (0..n)
        .map(|_| {
            let len = (next() % 12) as usize;
            (0..len)
                .map(|_| PIECES[(next() % PIECES.len() as u64) as usize])
                .collect()
        })
        .collect()
}

#[test]
fn fuzzed_input_never_panics_or_emits_unescaped_syntax() {
    let mut matched = 0;
    for q in random_queries(50_000, 0x5EA2C4) {
        let out = std::panic::catch_unwind(|| to_fts5(&q, &lexicon));
        let out = out.unwrap_or_else(|_| panic!("panicked on {q:?}"));
        match out {
            Ok(Compiled::Match(expr)) => {
                matched += 1;
                if let Err(why) = only_safe_syntax(&expr) {
                    panic!("{q:?} -> {expr:?}: {why}");
                }
            }
            Ok(Compiled::Morph(m)) => {
                if let Compiled::Match(expr) = m.expand(&CODES)
                    && let Err(why) = only_safe_syntax(&expr)
                {
                    panic!("{q:?} -> {expr:?}: {why}");
                }
            }
            Ok(Compiled::Nothing) => {}
            Err(e) => {
                // Error spans lie inside the query, on char boundaries.
                assert!(e.span.end <= q.len(), "{q:?}: {e:?}");
                assert!(q.is_char_boundary(e.span.start) && q.is_char_boundary(e.span.end));
            }
        }
    }
    assert!(matched > 10_000, "only {matched} queries compiled");
}

/// Notes search (#64) reads the same grammar against the Notes index's one `body` column.
#[test]
fn notes_queries_search_the_body_column() {
    let notes = |q: &str| expanded(compile_notes(&parse(q).unwrap()).unwrap());
    let table: &[(&str, &str)] = &[
        ("Laban heap", r#"body : "laban" AND body : "heap""#),
        (r#""a witness""#, r#"body : "a witness""#),
        ("witn*", r#"body : "witn" *"#),
        ("faith -works", r#"body : "faith" NOT body : "works""#),
        (
            "(faith OR hope) love",
            r#"(body : "faith" OR body : "hope") AND body : "love""#,
        ),
        // A Strong's number or Lemma is matched as written in the Note.
        ("G5485", r#"body : "g5485""#),
        ("lemma:ἀγάπη", r#"body : "αγαπη""#),
    ];
    for (q, want) in table {
        assert_eq!(notes(q), *want, "{q}");
    }
    // The Search index form is unchanged.
    assert_eq!(fts("G5485"), r#"strong : "g5485""#);
}

/// Morphology criteria (#145): `@` takes the words after it while they name criteria, and a
/// Strong's number or Lemma right before it is the same word's.
#[test]
fn morphology_criteria_compile_to_one_part_phrases() {
    let table: &[(&str, &str)] = &[
        // Every code that meets all the criteria, as a token of the `morph` column.
        (
            "@verb aorist imperative",
            r#"morph : "morph1" OR morph : "morph4""#,
        ),
        // The same criteria in any order, or through `morph:`, are the same query.
        (
            "@imperative aorist",
            r#"morph : "morph1" OR morph : "morph4""#,
        ),
        (
            "morph:aorist imperative",
            r#"morph : "morph1" OR morph : "morph4""#,
        ),
        (
            r#"morph:"aorist imperative""#,
            r#"morph : "morph1" OR morph : "morph4""#,
        ),
        // A Strong's number or Lemma before the `@` is in the same part, just before the
        // code (an extended number is followed by its base form), the Lemma just after.
        ("strong:G26 @noun genitive", r#"morph : "g26 morph3""#),
        ("G26 @noun gen", r#"morph : "g26 morph3""#),
        ("H1254A @qal", r#"morph : "h1254a h1254 morph5""#),
        ("lemma:agape @genitive", r#"morph : "morph3 αγαπη""#),
        (
            "lemma:bara @qal",
            r#"morph : "morph5 ברא" OR morph : "morph5 ברה""#,
        ),
        // Alongside words, phrases, OR and NOT.
        (
            r#""love one another" @imperative"#,
            r#"text : "love one another" AND (morph : "morph1" OR morph : "morph4")"#,
        ),
        (
            "love @aorist imperative -@indicative",
            r#"(text : "love" AND (morph : "morph1" OR morph : "morph4")) NOT morph : "morph2""#,
        ),
        // A word that is not Morphology ends the criteria.
        ("@genitive love", r#"morph : "morph3" AND text : "love""#),
        // Not attached: a word before, or a new `@`.
        (
            "G26 love @genitive",
            r#"strong : "g26" AND text : "love" AND morph : "morph3""#,
        ),
        // Nor across an explicit AND or a parenthesis: those join criteria of the Verse.
        (
            "G26 AND @genitive",
            r#"strong : "g26" AND morph : "morph3""#,
        ),
        (
            "@genitive AND G26",
            r#"morph : "morph3" AND strong : "g26""#,
        ),
        (
            "lemma:agape AND @genitive",
            r#"lemma : "αγαπη" AND morph : "morph3""#,
        ),
        ("(G26) @genitive", r#"strong : "g26" AND morph : "morph3""#),
        (
            "(love G26) @genitive",
            r#"text : "love" AND strong : "g26" AND morph : "morph3""#,
        ),
        ("G26 (@genitive)", r#"strong : "g26" AND morph : "morph3""#),
        (
            "G26 ((@genitive))",
            r#"strong : "g26" AND morph : "morph3""#,
        ),
        (
            "lemma:agape (@genitive)",
            r#"lemma : "αγαπη" AND morph : "morph3""#,
        ),
        (
            "(G26) (@genitive)",
            r#"strong : "g26" AND morph : "morph3""#,
        ),
        ("(G26 @genitive)", r#"morph : "g26 morph3""#),
        ("((G26 @genitive))", r#"morph : "g26 morph3""#),
        (
            "@verb @noun",
            r#"(morph : "morph1" OR morph : "morph2" OR morph : "morph4" OR morph : "morph5") AND morph : "morph3""#,
        ),
        // Two-word values, as words or joined.
        ("@consecutive imperfect", "<nothing>"),
        ("@perfect third masc sg", r#"morph : "morph5""#),
        // Criteria no code of this index meets.
        ("@optative", "<nothing>"),
        ("love OR @optative", r#"text : "love""#),
        ("love -@optative", r#"text : "love""#),
        ("lemma:zzz @noun", "<nothing>"),
    ];
    for (q, want) in table {
        assert_eq!(fts(q), *want, "{q}");
    }
    let q = parse("strong:G26 @noun genitive love").unwrap();
    let Query::And(parts) = q else { panic!() };
    assert!(matches!(&parts[0], Query::Morph { strong: Some(_), span, .. } if *span == (0..25)));
    let q = parse("@consecutive imperfect").unwrap();
    assert!(matches!(q, Query::Morph { span, .. } if span == (0..22)));
}

#[test]
fn morphology_errors_are_positioned() {
    let err = |q: &str| to_fts5(q, &NoLexicon).unwrap_err();
    let e = err("love @aorst");
    assert_eq!(e.kind, QueryErrorKind::UnknownMorph("aorst".into()));
    assert_eq!(e.span, 5..11);
    assert_eq!(err("love @").kind, QueryErrorKind::EmptyMorph);
    assert_eq!(err("love morph:").kind, QueryErrorKind::EmptyMorph);
    assert_eq!(
        err("@verb OR -@noun").kind,
        QueryErrorKind::ExcludedAlternative
    );
    assert_eq!(err("-@noun").kind, QueryErrorKind::OnlyExcluded);
    let e = compile_notes(&parse("love @verb").unwrap()).unwrap_err();
    assert_eq!(e.kind, QueryErrorKind::MorphInNotes);
    assert_eq!(e.span, 5..10);
}
