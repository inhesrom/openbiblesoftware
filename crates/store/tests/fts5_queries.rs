//! Compiled search queries (#57) run against a real SQLite FTS5 table shaped like the Search
//! index (#16 decision 3): `text`, `strong` (extended and base forms) and `lemma` columns.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use obs_model::normalize;
use obs_search::{Compiled, to_fts5};
use rusqlite::Connection;

/// (reference, text, strong's numbers, lemmas)
const DOCS: &[(&str, &str, &str, &str)] = &[
    (
        "Lk 2:40",
        "And the child grew, and waxed strong in spirit, filled with wisdom: and the grace of God was upon him.",
        "G5485",
        "χάρις",
    ),
    (
        "Lk 2:52",
        "And Jesus increased in wisdom and stature, and in favour with God and man.",
        "G5485 G2316",
        "χάρις θεός",
    ),
    (
        "1 Co 13:13",
        "And now abideth faith, hope, charity, these three; but the greatest of these is charity.",
        "G26 G4102 G1680",
        "ἀγάπη πίστις ἐλπίς",
    ),
    (
        "Ro 3:28",
        "Therefore we conclude that a man is justified by faith without the deeds of the law.",
        "G4102",
        "πίστις",
    ),
    (
        "Jas 2:24",
        "Ye see then how that by works a man is justified, and not by faith only.",
        "G2041 G4102",
        "ἔργον πίστις",
    ),
    (
        "Mt 22:39",
        "Thou shalt love thy neighbour as thyself.",
        "G25 G4139",
        "ἀγαπάω πλησίον",
    ),
    (
        "Gn 1:1",
        "In the beginning God created the heaven and the earth.",
        "H7225 H1254A H430",
        "רֵאשִׁית בָּרָא אֱלֹהִים",
    ),
    (
        "Jos 17:15",
        "get thee up to the wood country, and cut down for thyself there",
        "H1254B",
        "בָּרָא",
    ),
];

fn index() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE VIRTUAL TABLE idx USING fts5(text, strong, lemma, detail=full,
             tokenize = 'unicode61 remove_diacritics 0');",
    )
    .unwrap();
    for (i, (_, text, strong, lemma)) in DOCS.iter().enumerate() {
        // The index holds normalized words; `strong` holds extended and base forms.
        let words: Vec<String> = obs_model::tokenize(text)
            .into_iter()
            .map(|t| t.text)
            .collect();
        let mut strongs = Vec::new();
        for s in strong.split_whitespace() {
            let n: obs_scripture::StrongsNumber = s.parse().unwrap();
            strongs.push(n.to_string().to_lowercase());
            if n.is_extended() {
                strongs.push(n.base().to_string().to_lowercase());
            }
        }
        c.execute(
            "INSERT INTO idx (rowid, text, strong, lemma) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                i as i64,
                words.join(" "),
                strongs.join(" "),
                normalize(lemma)
            ],
        )
        .unwrap();
    }
    c
}

fn lexicon(t: &str) -> Vec<String> {
    match t {
        "agape" => vec!["ἀγάπη".into()],
        "pistis" => vec!["πίστις".into()],
        _ => vec![],
    }
}

fn search(c: &Connection, q: &str) -> Vec<&'static str> {
    let expr = match to_fts5(q, &lexicon).unwrap_or_else(|e| panic!("{q}: {e}")) {
        Compiled::Match(e) => e,
        Compiled::Nothing => return Vec::new(),
    };
    let mut stmt = c
        .prepare("SELECT rowid FROM idx WHERE idx MATCH ?1 ORDER BY rowid")
        .unwrap();
    let rows = stmt
        .query_map([&expr], |r| r.get::<_, i64>(0))
        .unwrap_or_else(|e| panic!("{q:?} -> {expr:?}: {e}"));
    rows.map(|r| DOCS[r.unwrap_or_else(|e| panic!("{q:?} -> {expr:?}: {e}")) as usize].0)
        .collect()
}

#[test]
fn every_form_matches_what_it_should() {
    let c = index();
    let table: &[(&str, &[&str])] = &[
        ("faith justified", &["Ro 3:28", "Jas 2:24"]),
        (r#""justified by faith""#, &["Ro 3:28"]),
        ("faith OR hope", &["1 Co 13:13", "Ro 3:28", "Jas 2:24"]),
        ("faith -works", &["1 Co 13:13", "Ro 3:28"]),
        ("faith NOT works", &["1 Co 13:13", "Ro 3:28"]),
        ("(faith OR wisdom) man", &["Lk 2:52", "Ro 3:28", "Jas 2:24"]),
        ("wis*", &["Lk 2:40", "Lk 2:52"]),
        ("FAVOUR", &["Lk 2:52"]),
        ("strong:G5485", &["Lk 2:40", "Lk 2:52"]),
        ("G26", &["1 Co 13:13"]),
        // Extended matches exactly; base matches every homograph.
        ("H1254A", &["Gn 1:1"]),
        ("strong:H1254", &["Gn 1:1", "Jos 17:15"]),
        ("lemma:ἀγάπη", &["1 Co 13:13"]),
        ("lemma:agape", &["1 Co 13:13"]),
        ("lemma:pistis -works", &["1 Co 13:13", "Ro 3:28"]),
        ("lemma:בָּרָא", &["Gn 1:1", "Jos 17:15"]),
        (r#"strong:G5485 "favour""#, &["Lk 2:52"]),
        ("lemma:zzz", &[]),
    ];
    for (q, want) in table {
        assert_eq!(search(&c, q), *want, "query {q:?}");
    }
}

#[test]
fn fuzzed_queries_are_always_valid_fts5() {
    const PIECES: &[&str] = &[
        "love",
        "faith",
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
        "OR",
        "AND",
        "NOT",
        "NEAR",
        "NEAR(",
        "strong:",
        "lemma:",
        "text:",
        "G26",
        "H1254A",
        "ἀγάπη",
        "בָּרָא",
        "\\",
        "\"\"",
        "x*y",
        "--",
        ")(",
        "\t",
        "λόγος",
        "🙂",
        "'",
        "%",
    ];
    let c = index();
    let mut state: u64 = 0x0B5_F75;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut ran = 0;
    for _ in 0..20_000 {
        let len = (next() % 10) as usize;
        let q: String = (0..len)
            .map(|_| PIECES[(next() % PIECES.len() as u64) as usize])
            .collect();
        if let Ok(Compiled::Match(expr)) = to_fts5(&q, &lexicon) {
            let n: Result<i64, _> = c.query_row(
                "SELECT count(*) FROM idx WHERE idx MATCH ?1",
                [&expr],
                |r| r.get(0),
            );
            if let Err(e) = n {
                panic!("{q:?} -> {expr:?}: {e}");
            }
            ran += 1;
        }
    }
    assert!(ran > 5_000, "only {ran} ran");
}
