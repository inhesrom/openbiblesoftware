use super::*;

const KJV_COUNTS: &str = include_str!("../tests/assets/verse_map/kjv2006-word-counts.txt");

/// The KJV verse list with real word counts, as chapters of `(verse, words)`.
fn kjv() -> BTreeMap<(Book, u16), Vec<(u16, u32)>> {
    let mut out = BTreeMap::new();
    for line in KJV_COUNTS.lines().filter(|l| !l.starts_with('#')) {
        let mut parts = line.split_whitespace();
        let book = Book::from_usfm(parts.next().unwrap()).unwrap();
        let chapter: u16 = parts.next().unwrap().parse().unwrap();
        let mut verses = Vec::new();
        let mut n = 0;
        for p in parts {
            match p.strip_prefix('T') {
                Some(t) => verses.push((0, t.parse().unwrap())),
                None => {
                    n += 1;
                    verses.push((n, p.parse().unwrap()));
                }
            }
        }
        out.insert((book, chapter), verses);
    }
    out
}

fn facts(bible: &BTreeMap<(Book, u16), Vec<(u16, u32)>>) -> Vec<VerseFacts> {
    bible
        .iter()
        .flat_map(|(&(b, c), vs)| {
            vs.iter().map(move |&(v, words)| VerseFacts {
                verse: Verse::new(b, c, v),
                last: v,
                words,
            })
        })
        .collect()
}

fn v(b: Book, c: u16, n: u16) -> Verse {
    Verse::new(b, c, n)
}

fn std1(b: Book, c: u16, n: u16) -> Option<Passage> {
    Some(Passage::verse(v(b, c, n)))
}

/// Renumber a chapter: `verses` lists (new number, words).
fn set(
    bible: &mut BTreeMap<(Book, u16), Vec<(u16, u32)>>,
    b: Book,
    c: u16,
    verses: Vec<(u16, u32)>,
) {
    bible.insert((b, c), verses);
}

#[test]
fn rules_load() {
    assert!(tvtms_rule_count() > 15_000);
}

/// Hebrew Mal 3:19-24 are the Standard Mal 4:1-6. TVTMS tells the Hebrew order from the
/// Greek by word counts (`Mal.3:23*2<Mal.3:22+Mal.3:24`), which in TAHOT's Hebrew are 13, 13
/// and 15: under the 25% margin. The verses are not in the Standard, so the rules run again
/// for them without the margin rather than leave them Unmapped.
#[test]
fn near_equal_word_counts_still_map_verses_the_standard_lacks() {
    let mut bible = kjv();
    set(
        &mut bible,
        Book::Malachi,
        3,
        (1..=24)
            .map(|n| {
                (
                    n,
                    if (22..=24).contains(&n) {
                        [13, 13, 15][usize::from(n - 22)]
                    } else {
                        20
                    },
                )
            })
            .collect(),
    );
    bible.remove(&(Book::Malachi, 4));
    let out = build_verse_map(&facts(&bible));
    let map = &out.map;
    assert_eq!(map.unmapped().count(), 0);
    assert_eq!(
        map.to_standard(v(Book::Malachi, 3, 19)),
        std1(Book::Malachi, 4, 1)
    );
    assert_eq!(
        map.to_standard(v(Book::Malachi, 3, 24)),
        std1(Book::Malachi, 4, 6)
    );
    assert_eq!(
        map.to_standard(v(Book::Malachi, 3, 18)),
        std1(Book::Malachi, 3, 18)
    );
    assert!(!map.is_absent(v(Book::Malachi, 4, 4)));
}

#[test]
fn kjv_maps_by_identity() {
    let bible = kjv();
    let f = facts(&bible);
    assert_eq!(f.len(), 31_218);
    let out = build_verse_map(&f);
    assert_eq!(out.map.unmapped().count(), 0);
    for e in out.map.entries() {
        assert_eq!(e.standard, Some(Passage::verse(e.verse)), "{}", e.verse);
    }
    // TVTMS gives Ps 119 a title slot; the KJV has no title there.
    assert_eq!(out.map.absent(), [v(Book::Psalms, 119, 0)]);
}

#[test]
fn hebrew_numbering_maps_onto_the_standard() {
    let mut bible = kjv();
    let k = bible.clone();
    let words = |c: (Book, u16)| -> Vec<u32> { k[&c].iter().map(|&(_, w)| w).collect() };
    // Gen 31:55 is Hebrew 32:1; Hebrew Gen 32 has 33 verses.
    let g31 = words((Book::Genesis, 31));
    let g32 = words((Book::Genesis, 32));
    set(
        &mut bible,
        Book::Genesis,
        31,
        (1..=54).zip(g31[..54].iter().copied()).collect(),
    );
    let heb32: Vec<u32> = std::iter::once(g31[54])
        .chain(g32.iter().copied())
        .collect();
    set(&mut bible, Book::Genesis, 32, (1..=33).zip(heb32).collect());
    // Psalm titles are verse 1 (Ps 9) or verses 1-2 (Ps 51) in Hebrew.
    let p9 = words((Book::Psalms, 9));
    set(
        &mut bible,
        Book::Psalms,
        9,
        (1..=21).zip(p9.iter().copied()).collect(),
    );
    let p51 = words((Book::Psalms, 51));
    let half = (p51[0] / 2).max(1);
    let heb51: Vec<u32> = [half, p51[0] - half]
        .into_iter()
        .chain(p51[1..].iter().copied())
        .collect();
    set(&mut bible, Book::Psalms, 51, (1..=21).zip(heb51).collect());

    let out = build_verse_map(&facts(&bible));
    let map = &out.map;
    assert_eq!(
        map.to_standard(v(Book::Genesis, 32, 1)),
        std1(Book::Genesis, 31, 55)
    );
    assert_eq!(
        map.to_standard(v(Book::Genesis, 32, 2)),
        std1(Book::Genesis, 32, 1)
    );
    assert_eq!(
        map.to_standard(v(Book::Genesis, 32, 33)),
        std1(Book::Genesis, 32, 32)
    );
    assert_eq!(
        map.to_native(v(Book::Genesis, 31, 55)),
        [v(Book::Genesis, 32, 1)]
    );
    assert_eq!(
        map.to_standard(v(Book::Psalms, 51, 3)),
        std1(Book::Psalms, 51, 1)
    );
    assert_eq!(
        map.to_standard(v(Book::Psalms, 51, 1)),
        std1(Book::Psalms, 51, 0)
    );
    assert_eq!(
        map.to_standard(v(Book::Psalms, 51, 2)),
        std1(Book::Psalms, 51, 0)
    );
    assert_eq!(
        map.to_standard(v(Book::Psalms, 51, 21)),
        std1(Book::Psalms, 51, 19)
    );
    assert_eq!(
        map.to_native(v(Book::Psalms, 51, 0)),
        [v(Book::Psalms, 51, 1), v(Book::Psalms, 51, 2)]
    );
    assert_eq!(
        map.first_native(v(Book::Psalms, 51, 1)),
        Some(v(Book::Psalms, 51, 3))
    );
    assert_eq!(
        map.to_standard(v(Book::Psalms, 9, 1)),
        std1(Book::Psalms, 9, 0)
    );
    assert_eq!(
        map.to_standard(v(Book::Psalms, 9, 21)),
        std1(Book::Psalms, 9, 20)
    );
    // Everything else still maps by identity; nothing is lost.
    assert_eq!(
        map.to_standard(v(Book::John, 3, 16)),
        std1(Book::John, 3, 16)
    );
    assert_eq!(map.unmapped().count(), 0);
    assert_eq!(map.absent(), [v(Book::Psalms, 119, 0)]);
    assert!(
        out.traditions.contains_key("Hebrew"),
        "{:?}",
        out.traditions
    );
}

#[test]
fn a_missing_verse_is_absent() {
    let mut bible = kjv();
    let mt17: Vec<(u16, u32)> = bible[&(Book::Matthew, 17)]
        .iter()
        .copied()
        .filter(|&(n, _)| n != 21)
        .collect();
    set(&mut bible, Book::Matthew, 17, mt17);
    let map = build_verse_map(&facts(&bible)).map;
    assert!(map.is_absent(v(Book::Matthew, 17, 21)));
    assert_eq!(map.to_native(v(Book::Matthew, 17, 21)), Vec::<Verse>::new());
    assert_eq!(
        map.to_standard(v(Book::Matthew, 17, 22)),
        std1(Book::Matthew, 17, 22)
    );
    assert_eq!(map.unmapped().count(), 0);
}

#[test]
fn greek_additions_inside_a_canonical_book_are_unmapped() {
    // Latin (Vulgate) Daniel 3 has 100 verses: 3:24-90 are the Song of the Three, which is
    // outside the 66-book canon; 3:91-100 are KJV 3:24-4:3. Latin Dan 4 is KJV 4:4-37.
    let mut bible = kjv();
    let d3: Vec<u32> = bible[&(Book::Daniel, 3)].iter().map(|&(_, w)| w).collect();
    let d4: Vec<u32> = bible[&(Book::Daniel, 4)].iter().map(|&(_, w)| w).collect();
    let mut lat3: Vec<u32> = d3[..23].to_vec();
    lat3.extend(std::iter::repeat_n(12, 67));
    lat3.extend(d3[23..].iter().copied());
    lat3.extend(d4[..3].iter().copied());
    set(&mut bible, Book::Daniel, 3, (1..=100).zip(lat3).collect());
    set(
        &mut bible,
        Book::Daniel,
        4,
        (1..=34).zip(d4[3..].iter().copied()).collect(),
    );
    let map = build_verse_map(&facts(&bible)).map;
    let unmapped: Vec<Verse> = map.unmapped().map(|e| e.verse).collect();
    assert_eq!(unmapped.len(), 67);
    assert_eq!(unmapped.first(), Some(&v(Book::Daniel, 3, 24)));
    assert_eq!(unmapped.last(), Some(&v(Book::Daniel, 3, 90)));
    assert_eq!(
        map.to_standard(v(Book::Daniel, 3, 91)),
        std1(Book::Daniel, 3, 24)
    );
    assert_eq!(
        map.to_standard(v(Book::Daniel, 3, 100)),
        std1(Book::Daniel, 4, 3)
    );
    assert_eq!(
        map.to_standard(v(Book::Daniel, 4, 1)),
        std1(Book::Daniel, 4, 4)
    );
    assert!(!map.is_absent(v(Book::Daniel, 4, 37)));
}

#[test]
fn verse_ranges_and_verses_beyond_the_standard() {
    let f = [
        VerseFacts {
            verse: v(Book::John, 1, 1),
            last: 2,
            words: 30,
        },
        VerseFacts {
            verse: v(Book::John, 1, 3),
            last: 3,
            words: 20,
        },
        VerseFacts {
            verse: v(Book::Jude, 1, 25),
            last: 26,
            words: 20,
        },
        VerseFacts {
            verse: v(Book::Jude, 1, 27),
            last: 27,
            words: 5,
        },
    ];
    let map = build_verse_map(&f).map;
    let p = |a, b| Passage::new(a, b).ok();
    assert_eq!(
        map.to_standard(v(Book::John, 1, 2)),
        p(v(Book::John, 1, 1), v(Book::John, 1, 2))
    );
    // Jude has 25 Standard verses: the range clamps, and 27 has no counterpart.
    assert_eq!(
        map.to_standard(v(Book::Jude, 1, 25)),
        std1(Book::Jude, 1, 25)
    );
    assert_eq!(map.to_standard(v(Book::Jude, 1, 27)), None);
    assert_eq!(map.unmapped().count(), 1);
    // Only books the Bible has count towards Absent verses.
    assert!(map.is_absent(v(Book::John, 1, 4)));
    assert!(!map.is_absent(v(Book::Genesis, 1, 1)));
}

#[test]
fn reference_and_test_parsing() {
    let r = parse_refs("Gen.5:32; 6:1").unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[1].start.verse(), Some(v(Book::Genesis, 6, 1)));
    let r = parse_refs("Gen.31:55--32:32").unwrap();
    assert_eq!(r[0].end.verse(), Some(v(Book::Genesis, 32, 32)));
    let r = parse_refs("3Jn.1:14!b").unwrap();
    assert_eq!(r[0].start.verse(), Some(v(Book::ThirdJohn, 1, 14)));
    let r = parse_refs("Gen.6:1.2").unwrap();
    assert_eq!(r[0].start.sub, Some(2));
    assert_eq!(parse_refs("Psa.3:Title").unwrap()[0].start.verse, 0);
    assert_eq!(parse_refs("Est.A:1").unwrap()[0].start.chapter, None);
    assert_eq!(parse_refs("S3Y.1:1").unwrap()[0].start.book, None);
    assert!(
        matches!(parse_test("Mal.4:5*2<Mal.4:4+Mal.4:6"), Test::Words { ref a, ref b, less: true } if a.len() == 1 && a[0].1 == 2 && b.len() == 2)
    );
    assert!(matches!(
        parse_test("Psa.51:TextBeforeV1=NotExist"),
        Test::Title(Some(Book::Psalms), Some(51), false)
    ));
    assert!(matches!(parse_test("Gen.32:33=Last"), Test::Last(_)));
    assert!(matches!(parse_test("nonsense"), Test::Unknown));
}
