use super::*;
use crate::versification::Standard;

fn parse(input: &str) -> PassageList {
    parse_reference(input, &Standard, None).unwrap_or_else(|e| panic!("{input:?}: {e}"))
}

fn short(input: &str) -> String {
    format_passage_list(&parse(input), &Standard, RefStyle::Short)
}

fn with_context(input: &str, context: Verse) -> String {
    let list = parse_reference(input, &Standard, Some(context))
        .unwrap_or_else(|e| panic!("{input:?}: {e}"));
    format_passage_list(&list, &Standard, RefStyle::Short)
}

fn error(input: &str) -> ReferenceError {
    match parse_reference(input, &Standard, None) {
        Ok(list) => panic!("{input:?} parsed as {list}"),
        Err(e) => e,
    }
}

/// Input as typed → canonical short form (Standard versification, no context).
const CASES: &[(&str, &str)] = &[
    // Spellings of every book.
    ("Gen 1:1", "Gen 1:1"),
    ("Gn 1:1", "Gen 1:1"),
    ("Ge 1:1", "Gen 1:1"),
    ("Genesis 1:1", "Gen 1:1"),
    ("GEN 1:1", "Gen 1:1"),
    ("Gen. 1:1", "Gen 1:1"),
    ("Ex 20:3", "Exod 20:3"),
    ("Exo 20:3", "Exod 20:3"),
    ("Lev 19:18", "Lev 19:18"),
    ("Lv 19:18", "Lev 19:18"),
    ("Num 6:24", "Num 6:24"),
    ("Deut 6:4", "Deut 6:4"),
    ("Dt 6:4", "Deut 6:4"),
    ("Deu 6:4", "Deut 6:4"),
    ("Josh 1:9", "Josh 1:9"),
    ("Jos 1:9", "Josh 1:9"),
    ("Judg 6:12", "Judg 6:12"),
    ("Jdg 6:12", "Judg 6:12"),
    ("Jg 6:12", "Judg 6:12"),
    ("Ruth 1:16", "Ruth 1:16"),
    ("Rt 1:16", "Ruth 1:16"),
    ("1 Sam 17:45", "1 Sam 17:45"),
    ("1Sa 17:45", "1 Sam 17:45"),
    ("I Samuel 17:45", "1 Sam 17:45"),
    ("First Samuel 17:45", "1 Sam 17:45"),
    ("1st Samuel 17:45", "1 Sam 17:45"),
    ("1 S 17:45", "1 Sam 17:45"),
    ("2 Sam 7:12", "2 Sam 7:12"),
    ("II Samuel 7:12", "2 Sam 7:12"),
    ("1 Kings 18:21", "1 Kgs 18:21"),
    ("1Ki 18:21", "1 Kgs 18:21"),
    ("1 Kgs 18:21", "1 Kgs 18:21"),
    ("2 Kgs 2:11", "2 Kgs 2:11"),
    ("1 Chr 16:34", "1 Chr 16:34"),
    ("1 Chronicles 16:34", "1 Chr 16:34"),
    ("2 Chron 7:14", "2 Chr 7:14"),
    ("Ezra 7:10", "Ezra 7:10"),
    ("Neh 8:10", "Neh 8:10"),
    ("Esth 4:14", "Esth 4:14"),
    ("Est 4:14", "Esth 4:14"),
    ("Job 19:25", "Job 19:25"),
    ("Ps 23:1", "Ps 23:1"),
    ("Psalm 23:1", "Ps 23:1"),
    ("Psalms 23:1", "Ps 23:1"),
    ("Psa 23:1", "Ps 23:1"),
    ("Pss 23:1", "Ps 23:1"),
    ("Prov 3:5", "Prov 3:5"),
    ("Pr 3:5", "Prov 3:5"),
    ("Prv 3:5", "Prov 3:5"),
    ("Eccl 3:1", "Eccl 3:1"),
    ("Ecc 3:1", "Eccl 3:1"),
    ("Qoh 3:1", "Eccl 3:1"),
    ("Song 2:4", "Song 2:4"),
    ("Song of Songs 2:4", "Song 2:4"),
    ("Song of Solomon 2:4", "Song 2:4"),
    ("SoS 2:4", "Song 2:4"),
    ("Sng 2:4", "Song 2:4"),
    ("Canticles 2:4", "Song 2:4"),
    ("Isa 53:5", "Isa 53:5"),
    ("Is 53:5", "Isa 53:5"),
    ("Jer 29:11", "Jer 29:11"),
    ("Lam 3:22", "Lam 3:22"),
    ("Ezek 36:26", "Ezek 36:26"),
    ("Ezk 36:26", "Ezek 36:26"),
    ("Dan 9:25", "Dan 9:25"),
    ("Hos 6:6", "Hos 6:6"),
    ("Joel 2:28", "Joel 2:28"),
    ("Amos 5:24", "Amos 5:24"),
    ("Ob 15", "Obad 1:15"),
    ("Jonah 2:9", "Jonah 2:9"),
    ("Jon 2:9", "Jonah 2:9"),
    ("Mic 6:8", "Mic 6:8"),
    ("Nah 1:7", "Nah 1:7"),
    ("Hab 2:4", "Hab 2:4"),
    ("Zeph 3:17", "Zeph 3:17"),
    ("Hag 2:9", "Hag 2:9"),
    ("Zech 9:9", "Zech 9:9"),
    ("Mal 3:10", "Mal 3:10"),
    ("Matt 5:3", "Matt 5:3"),
    ("Mt 5:3", "Matt 5:3"),
    ("Mat 5:3", "Matt 5:3"),
    ("Mark 1:15", "Mark 1:15"),
    ("Mk 1:15", "Mark 1:15"),
    ("Mrk 1:15", "Mark 1:15"),
    ("Mr 1:15", "Mark 1:15"),
    ("Luke 2:11", "Luke 2:11"),
    ("Lk 2:11", "Luke 2:11"),
    ("John 3:16", "John 3:16"),
    ("Jn 3:16", "John 3:16"),
    ("jn3:16", "John 3:16"),
    ("Jhn 3.16", "John 3:16"),
    ("Joh 3:16", "John 3:16"),
    ("Acts 2:38", "Acts 2:38"),
    ("Ac 2:38", "Acts 2:38"),
    ("Rom 8:28", "Rom 8:28"),
    ("Ro 8:28", "Rom 8:28"),
    ("Rm 8:28", "Rom 8:28"),
    ("1 Cor 13:4", "1 Cor 13:4"),
    ("1Co 13:4", "1 Cor 13:4"),
    ("2 Cor 5:17", "2 Cor 5:17"),
    ("Gal 2:20", "Gal 2:20"),
    ("Eph 2:8", "Eph 2:8"),
    ("Phil 4:13", "Phil 4:13"),
    ("Php 4:13", "Phil 4:13"),
    ("Col 1:15", "Col 1:15"),
    ("1 Thess 5:16", "1 Thess 5:16"),
    ("1Th 5:16", "1 Thess 5:16"),
    ("2 Thess 3:3", "2 Thess 3:3"),
    ("1 Tim 2:5", "1 Tim 2:5"),
    ("2 Tim 3:16", "2 Tim 3:16"),
    ("Titus 2:11", "Titus 2:11"),
    ("Tit 2:11", "Titus 2:11"),
    ("Phlm 6", "Phlm 1:6"),
    ("Philemon 6", "Phlm 1:6"),
    ("Phm 6", "Phlm 1:6"),
    ("Heb 11:1", "Heb 11:1"),
    ("Jas 1:5", "Jas 1:5"),
    ("James 1:5", "Jas 1:5"),
    ("1 Pet 5:7", "1 Pet 5:7"),
    ("1Pe 5:7", "1 Pet 5:7"),
    ("2 Pet 3:9", "2 Pet 3:9"),
    ("1 Jn 4:8", "1 Jn 4:8"),
    ("1 John 4:8", "1 Jn 4:8"),
    ("I John 4:8", "1 Jn 4:8"),
    ("1Jn 4:8", "1 Jn 4:8"),
    ("2 Jn 6", "2 Jn 1:6"),
    ("3 John 4", "3 Jn 1:4"),
    ("Jude 3", "Jude 1:3"),
    ("Rev 21:4", "Rev 21:4"),
    ("Re 21:4", "Rev 21:4"),
    ("Revelation 21:4", "Rev 21:4"),
    ("Revelations 21:4", "Rev 21:4"),
    ("Rv 21:4", "Rev 21:4"),
    // Ranges.
    ("Jn 3:16-18", "John 3:16-18"),
    ("Jn 3:16–18", "John 3:16-18"),
    ("Jn 3:16 - 18", "John 3:16-18"),
    ("Jn 3:16-4:2", "John 3:16-4:2"),
    ("Jn 3:16-16", "John 3:16"),
    ("Gen 1-3", "Gen 1-3"),
    ("Gen 1:1-2:3", "Gen 1:1-2:3"),
    ("Gen 1:1-31", "Gen 1"),
    ("Gen 1:1-50:26", "Gen"),
    ("Mal 3-4", "Mal 3-4"),
    ("Mal 4:6-Matt 1:1", "Mal 4:6-Matt 1:1"),
    ("Mal 4-Matt 2", "Mal 4-Matt 2"),
    ("Gen 50-Exod", "Gen 50-Exod 40"),
    ("Gen-Deut", "Gen-Deut"),
    ("Jude 24-25", "Jude 1:24-25"),
    ("Jude 1:1-25", "Jude"),
    ("Jude-Rev 2", "Jude-Rev 2"),
    ("3 Jn 14-Jude 2", "3 Jn 1:14-Jude 1:2"),
    // Whole chapters and books.
    ("Gen 1", "Gen 1"),
    ("Gen", "Gen"),
    ("Gal", "Gal"),
    ("Obad", "Obad"),
    ("Obad 1", "Obad 1:1"),
    // Lists.
    ("Jn 3:16-18; Rom 5:8", "John 3:16-18; Rom 5:8"),
    ("Rom 1-8; Gal", "Rom 1-8; Gal"),
    ("Jn 3:16, 18", "John 3:16; John 3:18"),
    ("Jn 3:16, 4:1", "John 3:16; John 4:1"),
    ("Jn 3:16; 4:1", "John 3:16; John 4:1"),
    ("Jn 3:16; 5", "John 3:16; John 5"),
    ("Gen 1, 3", "Gen 1; Gen 3"),
    ("Jn 3; 5", "John 3; John 5"),
    ("Rom 5:8, 10-12", "Rom 5:8; Rom 5:10-12"),
    ("Jn 3:16, 1 Jn 2:1", "John 3:16; 1 Jn 2:1"),
    ("  jn 3:16 ; ; rom 5:8 ;", "John 3:16; Rom 5:8"),
    // Lettered verses belong to their numbered verse.
    ("Jn 3:16a", "John 3:16"),
    ("Jn 3:16b-18", "John 3:16-18"),
    // Psalm title slots.
    ("Ps 3:0", "Ps 3:title"),
    ("Ps 3:title", "Ps 3:title"),
    ("Ps 51:0-2", "Ps 51:title-2"),
    ("Ps 51", "Ps 51"),
    ("Ps 51:1-19", "Ps 51:1-19"),
    ("Ps 50:23-51:title", "Ps 50:23-51:title"),
];

#[test]
fn table_parses_as_expected() {
    assert!(CASES.len() >= 100, "only {} cases", CASES.len());
    for (input, expected) in CASES {
        assert_eq!(short(input), *expected, "input {input:?}");
    }
}

#[test]
fn table_reparses_in_both_styles() {
    for (input, _) in CASES {
        let list = parse(input);
        for style in [RefStyle::Short, RefStyle::Long] {
            let text = format_passage_list(&list, &Standard, style);
            assert_eq!(parse(&text), list, "{input:?} formatted as {text:?}");
        }
    }
}

#[test]
fn every_book_and_chapter_round_trips() {
    let v = &Standard;
    let mut passages = Vec::new();
    for book in Book::ALL {
        passages.extend(Passage::book(v, book));
        if let (Some(last), Some(next)) = (v.last_verse_of_book(book), book.next()) {
            passages.extend(
                v.first_verse_of_book(next)
                    .and_then(|f| Passage::new(last, f).ok()),
            );
        }
        for chapter in 1..=v.chapter_count(book) {
            let (Some(first), Some(last)) =
                (v.first_verse(book, chapter), v.last_verse_of(book, chapter))
            else {
                panic!("{book} {chapter} missing");
            };
            passages.extend(Passage::chapter(v, book, chapter));
            passages.push(Passage::verse(first));
            passages.push(Passage::verse(last));
            if last.verse >= 3 {
                passages.extend(Passage::new(Verse::new(book, chapter, 2), last).ok());
            }
            if let Some(next) = v.next_verse(last) {
                passages.extend(Passage::new(last, next).ok());
            }
        }
    }
    assert!(passages.len() > 6000);
    for p in passages {
        for style in [RefStyle::Short, RefStyle::Long] {
            let text = format_passage(p, v, style);
            let back = parse(&text);
            assert_eq!(back.passages(), [p], "{text:?}");
        }
    }
}

#[test]
fn long_style() {
    let list = parse("Song 2:4; 1 Jn 4:8; Ps 51:0-2; Obad; Mal 4:6-Matt 1:1");
    assert_eq!(
        format_passage_list(&list, &Standard, RefStyle::Long),
        "Song of Songs 2:4; 1 John 4:8; Psalms 51:title-2; Obadiah; Malachi 4:6-Matthew 1:1"
    );
}

#[test]
fn relative_forms_use_the_context_verse() {
    let jn = Verse::new(Book::John, 3, 16);
    assert_eq!(with_context("5", jn), "John 3:5");
    assert_eq!(with_context("2:4", jn), "John 2:4");
    assert_eq!(with_context("5-7", jn), "John 3:5-7");
    assert_eq!(with_context("4:1-3", jn), "John 4:1-3");
    assert_eq!(with_context("8, 10", jn), "John 3:8; John 3:10");
    assert_eq!(with_context("18; Rom 5:8", jn), "John 3:18; Rom 5:8");
    assert_eq!(with_context("Gen 2", jn), "Gen 2");
    assert_eq!(with_context("5", Verse::new(Book::Jude, 1, 1)), "Jude 1:5");
    assert_eq!(
        with_context("51:0", Verse::new(Book::Psalms, 3, 1)),
        "Ps 51:title"
    );
}

#[test]
fn errors_carry_kind_and_position() {
    use ReferenceErrorKind as K;
    let cases: Vec<(&str, K, Range<usize>)> = vec![
        ("", K::Empty, 0..0),
        (" ; ", K::Empty, 0..3),
        ("Xyz 1:1", K::UnknownBook("Xyz".into()), 0..3),
        (
            "Ph 1:1",
            K::AmbiguousBook {
                text: "Ph".into(),
                candidates: vec![Book::Philippians, Book::Philemon],
            },
            0..2,
        ),
        ("3:16", K::MissingBook, 0..1),
        (
            "Jn 3:37",
            K::VerseOutOfRange {
                book: Book::John,
                chapter: 3,
                verse: 37,
                last: 36,
            },
            5..7,
        ),
        (
            "Jn 22:1",
            K::ChapterOutOfRange {
                book: Book::John,
                chapter: 22,
                last: 21,
            },
            3..5,
        ),
        (
            "Gen 0",
            K::ChapterOutOfRange {
                book: Book::Genesis,
                chapter: 0,
                last: 50,
            },
            4..5,
        ),
        (
            "Ps 1:0",
            K::NoTitle {
                book: Book::Psalms,
                chapter: 1,
            },
            5..6,
        ),
        ("Jn 3:18-16", K::Reversed, 0..10),
        ("Jn 3:16 #", K::UnexpectedChar('#'), 8..9),
        ("Jn 3:99999999", K::NumberTooLarge, 5..13),
        ("Jn 3:", K::MissingNumber, 5..5),
        ("Jn 3:16-", K::MissingNumber, 8..8),
        ("Jn 3:16 Rom", K::UnexpectedToken, 8..11),
    ];
    for (input, kind, span) in cases {
        let e = error(input);
        assert_eq!((e.kind, e.span), (kind, span), "input {input:?}");
    }
    assert!(matches!(error("Jo 1:1").kind, K::AmbiguousBook { .. }));
    assert_eq!(
        error("Jn 3:37").to_string(),
        "John 3:37 does not exist (John 3 has 36 verses) (at 5..7)"
    );
}

/// A Bible whose Malachi has three chapters (Hebrew numbering: English Mal 4 is Mal 3:19-24).
struct HebrewMalachi;

impl Versification for HebrewMalachi {
    fn chapter_count(&self, book: Book) -> u16 {
        if book == Book::Malachi {
            3
        } else {
            Standard.chapter_count(book)
        }
    }

    fn last_verse(&self, book: Book, chapter: u16) -> Option<u16> {
        match (book, chapter) {
            (Book::Malachi, 3) => Some(24),
            (Book::Malachi, 4..) => None,
            _ => Standard.last_verse(book, chapter),
        }
    }

    fn has_title(&self, book: Book, chapter: u16) -> bool {
        Standard.has_title(book, chapter)
    }
}

#[test]
fn reads_in_the_supplied_versification() {
    let hebrew: &dyn Versification = &HebrewMalachi;
    let list = parse_reference("Mal 3:24", hebrew, None).unwrap();
    assert_eq!(
        format_passage_list(&list, hebrew, RefStyle::Short),
        "Mal 3:24"
    );
    assert!(matches!(
        parse_reference("Mal 3:24", &Standard, None)
            .unwrap_err()
            .kind,
        ReferenceErrorKind::VerseOutOfRange { last: 18, .. }
    ));
    assert!(matches!(
        parse_reference("Mal 4:1", hebrew, None).unwrap_err().kind,
        ReferenceErrorKind::ChapterOutOfRange { last: 3, .. }
    ));
    let whole = parse_reference("Mal", hebrew, None).unwrap();
    assert_eq!(whole.passages()[0].end(), Verse::new(Book::Malachi, 3, 24));
    assert_eq!(format_passage_list(&whole, hebrew, RefStyle::Short), "Mal");
}

#[test]
fn book_keys_have_no_conflicts() {
    let (_, conflicts) = build_keys();
    assert!(
        conflicts.is_empty(),
        "keys claimed by two books: {conflicts:?}"
    );
}

#[test]
fn hostile_input_never_panics() {
    for input in [
        "-",
        ":",
        "::",
        "1",
        "1 1",
        "I",
        "1st",
        "Gen-",
        "-Gen",
        "Gen:1",
        "Gen 1:",
        "Gen 1-:",
        "Gen 1:1--2",
        "Ps 3:title-title",
        "title",
        "1 2 3",
        "Jn ,",
        "Song of",
        "\u{5d1}\u{5e8}",
        "Gen 65535:65535",
        "Jn 3:16a b c",
        "1 Jn 1 Jn",
    ] {
        let _ = parse_reference(input, &Standard, None);
        let _ = parse_reference(input, &Standard, Some(Verse::new(Book::John, 3, 16)));
    }
}
