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

/// A Search index's Morphology codes and their tokens, and its words.
struct Codes(&'static [(&'static str, &'static str)]);

/// The words of the test index, for stemming.
const WORDS: &[&str] = &[
    "do", "doest", "doth", "faith", "go", "goeth", "live", "lived", "liveth", "love", "loved",
    "lovest", "loveth", "loving", "work", "works", "worketh",
];

impl IndexVocabulary for Codes {
    fn morph_tokens(&self, criteria: &MorphCriteria) -> Vec<String> {
        self.0
            .iter()
            .filter(|(_, code)| criteria.matches_code(code))
            .map(|(t, _)| (*t).to_string())
            .collect()
    }

    fn words_with_stem(&self, stem: &str) -> Vec<String> {
        WORDS
            .iter()
            .filter(|w| crate::stem::stem(w) == stem)
            .map(|w| (*w).to_string())
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
        Compiled::PerIndex(q) => match q.plan(&CODES) {
            Plan::Match(s) => s,
            Plan::Nothing => "<nothing>".into(),
            p => format!("{p:?}"),
        },
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
        // `text:` is the words as written, even shaped like a Strong's number (#257).
        ("text:G26", r#"text : "g26""#),
        ("text:H1254A", r#"text : "h1254a""#),
        ("text:G26*", r#"text : "g26" *"#),
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
            ' ' | ':' | '*' | ',' => {}
            c if c.is_ascii_digit() => {}
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
                if ![
                    "AND", "OR", "NOT", "NEAR", "text", "strong", "lemma", "morph",
                ]
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

/// Every MATCH expression in a plan.
pub(crate) fn plan_matches(plan: &Plan) -> Vec<String> {
    match plan {
        Plan::Match(s) => vec![s.clone()],
        Plan::Nothing => Vec::new(),
        Plan::All { all, not } => all.iter().chain(not).flat_map(plan_matches).collect(),
        Plan::Any(any) => any.iter().flat_map(plan_matches).collect(),
        Plan::Before { candidates, .. } => vec![candidates.clone()],
        Plan::Within { left, right, .. } => {
            let mut out = plan_matches(left);
            out.extend(plan_matches(right));
            out
        }
    }
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
        "NEAR/5",
        "NEAR/",
        "BEFORE/3",
        "BEFORE/0",
        "WITHIN",
        "2",
        "verses",
        "words",
        "/",
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
            Ok(Compiled::PerIndex(m)) => {
                for expr in plan_matches(&m.plan(&CODES)) {
                    if let Err(why) = only_safe_syntax(&expr) {
                        panic!("{q:?} -> {expr:?}: {why}");
                    }
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
        // A cancelled double negation keeps its parenthesis, and a NOT between them parts
        // them too.
        (
            "G26 NOT NOT (@genitive)",
            r#"strong : "g26" AND morph : "morph3""#,
        ),
        (
            "G26 --(@genitive)",
            r#"strong : "g26" AND morph : "morph3""#,
        ),
        (
            "NOT NOT (G26) @genitive",
            r#"strong : "g26" AND morph : "morph3""#,
        ),
        (
            "--(lemma:agape) @genitive",
            r#"lemma : "αγαπη" AND morph : "morph3""#,
        ),
        (
            "G26 NOT NOT @genitive",
            r#"strong : "g26" AND morph : "morph3""#,
        ),
        (
            "NOT NOT G26 @genitive",
            r#"strong : "g26" AND morph : "morph3""#,
        ),
        ("--(G26 @genitive)", r#"morph : "g26 morph3""#),
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

fn plan(q: &str) -> Plan {
    match to_fts5(q, &lexicon).unwrap_or_else(|e| panic!("{q}: {e}")) {
        Compiled::PerIndex(q) => q.plan(&CODES),
        Compiled::Match(s) => Plan::Match(s),
        Compiled::Nothing => Plan::Nothing,
    }
}

fn pattern(words: &[&str], prefix: bool) -> Pattern {
    Pattern {
        words: words.iter().map(|w| (*w).to_string()).collect(),
        prefix,
    }
}

#[test]
fn near_and_within_words_compile_to_fts5_near() {
    let table: &[(&str, &str)] = &[
        ("faith NEAR/5 works", r#"text : NEAR("faith" "works", 5)"#),
        (
            "faith WITHIN 5 words works",
            r#"text : NEAR("faith" "works", 5)"#,
        ),
        (
            "faith WITHIN 1 word works",
            r#"text : NEAR("faith" "works", 1)"#,
        ),
        (
            r#""by faith" NEAR/3 work*"#,
            r#"text : NEAR("by faith" "work" *, 3)"#,
        ),
        (
            "(faith OR hope) NEAR/2 love",
            r#"text : NEAR("faith" "love", 2) OR text : NEAR("hope" "love", 2)"#,
        ),
        // Tighter than AND and OR.
        (
            "grace faith NEAR/5 works",
            r#"text : "grace" AND text : NEAR("faith" "works", 5)"#,
        ),
        (
            "grace OR faith NEAR/5 works",
            r#"text : "grace" OR text : NEAR("faith" "works", 5)"#,
        ),
        (
            "faith NEAR/5 works -dead",
            r#"text : NEAR("faith" "works", 5) NOT text : "dead""#,
        ),
        // Operators only in these forms; otherwise words. Dangling operators go.
        ("NEAR works", r#"text : "near" AND text : "works""#),
        (
            "faith near/5 works",
            r#"text : "faith" AND text : "near 5" AND text : "works""#,
        ),
        ("faith NEAR/5", r#"text : "faith""#),
        ("NEAR/5 works", r#"text : "works""#),
        (
            "faith NEAR/5 OR works",
            r#"text : "faith" OR text : "works""#,
        ),
        (
            "faith WITHIN works",
            r#"text : "faith" AND text : "within" AND text : "works""#,
        ),
        (
            "faith WITHIN 2 chapters works",
            r#"text : "faith" AND text : "within" AND text : "2" AND text : "chapters" AND text : "works""#,
        ),
    ];
    for (q, want) in table {
        assert_eq!(fts(q), *want, "query {q:?}");
    }
}

#[test]
fn before_is_planned_as_near_candidates_checked_for_order() {
    assert_eq!(
        plan("faith BEFORE/3 works"),
        Plan::Before {
            candidates: r#"text : NEAR("faith" "works", 3)"#.into(),
            first: vec![pattern(&["faith"], false)],
            second: vec![pattern(&["works"], false)],
            words: 3,
        }
    );
    assert_eq!(
        plan(r#"grace "by faith" BEFORE/0 work*"#),
        Plan::All {
            all: vec![
                Plan::Match(r#"text : "grace""#.into()),
                Plan::Before {
                    candidates: r#"text : NEAR("by faith" "work" *, 0)"#.into(),
                    first: vec![pattern(&["by", "faith"], false)],
                    second: vec![pattern(&["work"], true)],
                    words: 0,
                },
            ],
            not: vec![],
        }
    );
}

#[test]
fn within_verses_is_planned_as_a_pass_over_neighbouring_documents() {
    let within = |left: &str, right: &str, documents| Plan::Within {
        left: Box::new(Plan::Match(left.into())),
        right: Box::new(Plan::Match(right.into())),
        documents,
    };
    let want = within(r#"text : "faith""#, r#"text : "works""#, 2);
    assert_eq!(plan("faith WITHIN 2 verses works"), want);
    assert_eq!(plan("faith WITHIN 2 verse works"), want);
    assert_eq!(plan("faith WITHIN 2 paragraphs works"), want);
    assert_eq!(plan("faith WITHIN 2 Verses works"), want);
    // 0 is the same Verse, like 1.
    assert_eq!(
        plan("faith WITHIN 0 verses works"),
        within(r#"text : "faith""#, r#"text : "works""#, 1)
    );
    // Any query on either side, in parentheses.
    assert_eq!(
        plan("(faith -dead) WITHIN 3 verses G2041"),
        within(
            r#"text : "faith" NOT text : "dead""#,
            r#"strong : "g2041""#,
            3
        )
    );
    // Combined with other terms.
    assert_eq!(
        plan("grace (faith WITHIN 2 verses works) -law"),
        Plan::All {
            all: vec![
                Plan::Match(r#"text : "grace""#.into()),
                within(r#"text : "faith""#, r#"text : "works""#, 2),
            ],
            not: vec![Plan::Match(r#"text : "law""#.into())],
        }
    );
    assert_eq!(
        plan("hope OR faith WITHIN 2 verses works"),
        Plan::Any(vec![
            Plan::Match(r#"text : "hope""#.into()),
            within(r#"text : "faith""#, r#"text : "works""#, 2),
        ])
    );
    // A side that matches nothing makes the whole of it match nothing.
    assert_eq!(plan("lemma:zzz WITHIN 2 verses works"), Plan::Nothing);
}

/// Morphology criteria written directly after a Strong's number or Lemma describe that same
/// word on either side of a proximity operator: `G26 @noun WITHIN 2 verses love` is
/// `(G26 @noun) WITHIN 2 verses love`, not `G26 AND (@noun WITHIN 2 verses love)`.
#[test]
fn morphology_attaches_before_proximity_joins() {
    let same = [
        (
            "G26 @noun WITHIN 2 verses love",
            "(G26 @noun) WITHIN 2 verses love",
        ),
        (
            "love WITHIN 2 verses G26 @noun",
            "love WITHIN 2 verses (G26 @noun)",
        ),
        (
            "strong:G26 @noun genitive WITHIN 3 verses lemma:ἀγάπη @noun",
            "(strong:G26 @noun genitive) WITHIN 3 verses (lemma:ἀγάπη @noun)",
        ),
        (
            "G26 morph:noun WITHIN 2 verses love",
            "(G26 @noun) WITHIN 2 verses love",
        ),
        (
            "grace G26 @noun WITHIN 2 verses love -law",
            "grace ((G26 @noun) WITHIN 2 verses love) -law",
        ),
        // Boundaries stay: AND, a parenthesis and a negation keep the criteria apart.
        (
            "G26 AND @noun WITHIN 2 verses love",
            "G26 AND (@noun WITHIN 2 verses love)",
        ),
        (
            "G26 (@noun) WITHIN 2 verses love",
            "G26 AND ((@noun) WITHIN 2 verses love)",
        ),
        (
            "(G26) @noun WITHIN 2 verses love",
            "G26 AND (@noun WITHIN 2 verses love)",
        ),
        (
            "NOT NOT G26 @noun WITHIN 2 verses love",
            "G26 AND (@noun WITHIN 2 verses love)",
        ),
    ];
    for (q, grouped) in same {
        assert_eq!(plan(q), plan(grouped), "{q}");
    }
    assert_ne!(
        plan("G26 @noun WITHIN 2 verses love"),
        plan("G26 AND (@noun WITHIN 2 verses love)")
    );
    let Query::Within { left, right, .. } = parse("G26 @noun WITHIN 2 verses love").unwrap() else {
        panic!()
    };
    assert!(matches!(*left, Query::Morph { strong: Some(_), ref span, .. } if *span == (0..9)));
    assert!(matches!(*right, Query::Word { .. }));
    let Query::Within { left, right, .. } = parse("love WITHIN 2 verses G26 @noun").unwrap() else {
        panic!()
    };
    assert!(matches!(*left, Query::Word { .. }));
    assert!(matches!(*right, Query::Morph { strong: Some(_), ref span, .. } if *span == (21..30)));
}

#[test]
fn proximity_errors_are_positioned() {
    let err = |q: &str| to_fts5(q, &NoLexicon).unwrap_err();
    let e = err("G26 NEAR/3 love");
    assert_eq!(e.kind, QueryErrorKind::ProximityOperand);
    assert_eq!(e.span, 0..3);
    let e = err("love BEFORE/3 (faith hope)");
    assert_eq!(e.kind, QueryErrorKind::ProximityOperand);
    let e = err("love NEAR/3 -faith");
    assert_eq!(e.kind, QueryErrorKind::ProximityOperand);
    assert_eq!(e.span, 12..18);
    let e = err("-faith WITHIN 2 verses works");
    assert_eq!(e.kind, QueryErrorKind::OnlyExcluded);
    // Notes: NEAR works; BEFORE and WITHIN … verses do not.
    let notes = |q: &str| compile_notes(&parse(q).unwrap());
    assert_eq!(
        notes("faith NEAR/2 works"),
        Ok(Compiled::Match(r#"body : NEAR("faith" "works", 2)"#.into()))
    );
    for q in [
        "faith BEFORE/2 works",
        "grace (faith WITHIN 2 verses works)",
    ] {
        let e = notes(q).unwrap_err();
        assert_eq!(e.kind, QueryErrorKind::ProximityInNotes, "{q}");
    }
    assert!(
        notes("faith BEFORE/2 works")
            .unwrap_err()
            .to_string()
            .contains("not Notes")
    );
}

#[test]
fn word_forms_match_every_word_of_the_stem_in_the_index() {
    let forms = |q: &str| {
        let parsed = parse(q).unwrap().with_word_forms();
        expanded(compile(&parsed, &lexicon).unwrap())
    };
    let love = r#"text : "love" OR text : "loved" OR text : "lovest" OR text : "loveth" OR text : "loving""#;
    assert_eq!(forms("love"), love);
    assert_eq!(forms("loveth"), love);
    assert_eq!(forms("Loving"), love);
    // Off by default.
    assert_eq!(fts("love"), r#"text : "love""#);
    // Quoted words, prefixes, Strong's numbers and Lemmas are exact.
    assert_eq!(forms(r#""love""#), r#"text : "love""#);
    assert_eq!(forms(r#"text:"love""#), r#"text : "love""#);
    assert_eq!(forms(r#"text:"love one""#), r#"text : "love one""#);
    assert_eq!(forms("lov*"), r#"text : "lov" *"#);
    assert_eq!(forms("G26"), r#"strong : "g26""#);
    assert_eq!(forms("lemma:agape"), r#"lemma : "αγαπη""#);
    // Inside other forms.
    assert_eq!(
        forms("faith works"),
        r#"text : "faith" AND (text : "work" OR text : "works" OR text : "worketh")"#
    );
    assert_eq!(
        forms("faith NEAR/3 works"),
        r#"text : NEAR("faith" "work", 3) OR text : NEAR("faith" "works", 3) OR text : NEAR("faith" "worketh", 3)"#
    );
    // Short words have forms too.
    assert_eq!(
        forms("do"),
        r#"text : "do" OR text : "doest" OR text : "doth""#
    );
    assert_eq!(forms("goeth"), r#"text : "go" OR text : "goeth""#);
    // A field-qualified bare word is stemmed like a bare word.
    assert_eq!(forms("text:loveth"), love);
    // A word with no forms in this index matches nothing there.
    assert_eq!(forms("grace"), "<nothing>");
    assert_eq!(forms("grace OR love"), format!("({love})"));
    // Notes keep the word as written.
    let parsed = parse("loveth").unwrap().with_word_forms();
    assert_eq!(
        compile_notes(&parsed),
        Ok(Compiled::Match(r#"body : "loveth""#.into()))
    );
}
