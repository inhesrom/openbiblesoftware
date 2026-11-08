//! Synthetic Resources for the bench's feature scenarios (#183), sized like the real data the
//! features meet, so CI (which has no Starter library) measures realistic work: whole Bibles
//! to search, a tagged Hebrew Torah and its Lexicon for the Exegetical Guide, a People &
//! places index with located places for the Atlas, topical indexes for the Topic Guide and a
//! Get Resources catalog as long as the live SWORD repositories'.
//!
//! Everything is deterministic (a fixed xorshift seed per Resource).

use std::path::Path;

use obs_engine::get_bibles::{ListingSource, RepoRef, ResourceCatalog, ResourceListing};
use obs_model::{
    Entity, EntityKind, Key, KeyScheme, Manifest, MorphemePart, ParaStyle, PlaceLocation, Resource,
    ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind,
};
use obs_scripture::{Book, Passage, RefStyle, Standard, Verse, Versification, format_passage};

/// The books of the reading bench's Bible ([`super::synthetic_bible`]): every Resource that
/// follows a Passage here is keyed to them.
pub(crate) const TORAH: [Book; 5] = [
    Book::Genesis,
    Book::Exodus,
    Book::Leviticus,
    Book::Numbers,
    Book::Deuteronomy,
];

/// Places in the People & places index: about as many as OpenBible.info locates.
pub(crate) const PLACES: usize = 1_300;

/// Places the busiest chapter of each book names (Joshua 15 names about 120).
pub(crate) const BUSY_CHAPTER_PLACES: usize = 120;

/// The chapter of each book that names [`BUSY_CHAPTER_PLACES`] places.
pub(crate) const BUSY_CHAPTER: u16 = 15;

/// Rows of the Get Resources catalog. The live SWORD repositories list about 2,200 modules
/// (eBible.org 1,530, CrossWire 428, Attic 88, Wycliffe 44, … in November 2026); this leaves
/// headroom.
pub(crate) const CATALOG_ROWS: usize = 3_000;

/// Topics of each topical index besides the long ones (Nave's has about 5,000).
const TOPICS: usize = 1_000;

/// The long topics, with (subtopics, Passages per subtopic): Nave's PRAYER runs to hundreds of
/// citations.
pub(crate) const LONG_TOPICS: [(&str, usize, usize); 2] = [("Prayer", 60, 20), ("Faith", 30, 15)];

/// A deterministic xorshift generator.
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n.max(1)
    }

    pub(super) fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let i = usize::try_from(self.below(items.len() as u64)).unwrap_or(0);
        &items[i]
    }
}

const SYLLABLES: &[&str] = &[
    "ab", "ra", "ham", "el", "i", "sa", "ac", "ja", "cob", "mo", "ses", "ben", "min", "na", "than",
    "she", "ba", "le", "vi", "ju", "dah", "gad", "ash", "er", "zeb", "u", "lun", "dan", "naph",
    "ta", "li", "jo", "seph", "re", "sim", "e", "on",
];

pub(super) const WORDS: &[&str] = &[
    "and", "the", "LORD", "said", "unto", "of", "that", "they", "me", "an", "every", "man", "his",
    "ye", "shall", "my", "this", "is", "which", "in", "to", "be", "for", "with",
];

pub(super) fn made_up_word(rng: &mut Rng, syllables: u64) -> String {
    (0..syllables).map(|_| *rng.pick(SYLLABLES)).collect()
}

pub(super) fn capitalized(word: &str) -> String {
    let mut c = word.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// An English Bible of `books` with verses of `words` words (6–17 make short verses; a real
/// English Bible averages about 25) and a paragraph every 3–8 verses. With `strongs`, seven
/// words in ten are Tagged words with a Strong's number only, as in the KJV with Strong's;
/// otherwise untagged. Several of these make a whole-Bible search across Bibles.
pub(crate) fn english_bible(
    id: &str,
    abbreviation: &str,
    books: &[Book],
    words: std::ops::Range<u64>,
    strongs: bool,
    seed: u64,
) -> Option<Resource> {
    let mut m = Manifest::new(
        id,
        format!("Bench Bible {abbreviation}"),
        abbreviation,
        ResourceType::Bible,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let mut rng = Rng(seed);
    for &book in books {
        let unit = UnitId::book(book);
        let mut text = String::new();
        let mut verses = Vec::new();
        let mut paras = Vec::new();
        let mut tagged = Vec::new();
        // Strong's numbers of the book's language: Hebrew 1–8674, Greek 1–5624.
        let (prefix, numbers) = match book.testament() {
            obs_scripture::Testament::Old => ('H', 8674),
            obs_scripture::Testament::New => ('G', 5624),
        };
        for chapter in 1..=Standard.chapter_count(book) {
            let last = Standard.last_verse(book, chapter).unwrap_or(1);
            let mut para_start = text.len();
            let mut para_left = 3 + rng.below(6);
            for v in 1..=last {
                if text.len() > para_start {
                    text.push(' ');
                }
                let start = text.len();
                let count = words.start + rng.below(words.end - words.start);
                let verse: Vec<String> = (0..count)
                    .map(|_| {
                        if rng.below(10) < 6 {
                            (*rng.pick(WORDS)).to_string()
                        } else {
                            let n = 1 + rng.below(3);
                            made_up_word(&mut rng, n)
                        }
                    })
                    .collect();
                if strongs {
                    let mut at = start;
                    for w in &verse {
                        if rng.below(10) < 7
                            && let Ok(n) = format!("{prefix}{}", 1 + rng.below(numbers)).parse()
                        {
                            tagged.push((at..at + w.len(), MorphemePart::new(n)));
                        }
                        at += w.len() + 1;
                    }
                }
                text.push_str(&verse.join(" "));
                text.push('.');
                verses.push((start..text.len(), Verse::new(book, chapter, v)));
                para_left = para_left.saturating_sub(1);
                if para_left == 0 || v == last {
                    paras.push(para_start..text.len());
                    text.push('\n');
                    para_start = text.len();
                    para_left = 3 + rng.below(6);
                }
            }
        }
        b.add_unit(
            unit.clone(),
            UnitKind::BibleBook,
            Some(book.name().into()),
            text,
        );
        for r in paras {
            b.add_span(
                &unit,
                r,
                SpanKind::Para {
                    style: ParaStyle::Prose,
                },
            );
        }
        for (r, v) in verses {
            b.add_verse(&unit, r, v, v.verse);
        }
        for (r, part) in tagged {
            b.add_span(&unit, r, SpanKind::TaggedWord { parts: vec![part] });
        }
    }
    b.build().ok()
}

/// Hebrew consonants and vowel points the Lemmas are made of.
const CONSONANTS: &[char] = &[
    'א', 'ב', 'ג', 'ד', 'ה', 'ו', 'ז', 'ח', 'ט', 'י', 'כ', 'ל', 'מ', 'נ', 'ס', 'ע', 'פ', 'צ', 'ק',
    'ר', 'ש', 'ת',
];
const POINTS: &[char] = &[
    '\u{05B8}', '\u{05B7}', '\u{05B5}', '\u{05B4}', '\u{05B9}', '\u{05B0}',
];
/// STEPBible Hebrew Morphology codes, as TAHOT writes them.
const MORPHS: &[&str] = &[
    "HNcmsa", "HNcfsa", "HNcmpa", "HNcfpc", "HVqp3ms", "HVqw3ms", "HVhp3ms", "HVqi3mp", "HAamsa",
    "HNpm", "HTo", "HR",
];
const GLOSSES: &[&str] = &[
    "to create",
    "God",
    "land",
    "heavens",
    "man",
    "day",
    "light",
    "water",
    "to say",
    "son",
    "all",
    "to go",
    "house",
    "word",
    "father",
    "to give",
    "people",
    "to see",
    "hand",
    "city",
];

/// Strong's numbers of the Hebrew Bible's words; each has a Lexicon entry.
const LEMMAS: u32 = 400;

/// The Lemma of Strong's number H`n`: three pointed consonants, made from `n`.
fn lemma(n: u32) -> String {
    let mut out = String::new();
    let mut k = n.wrapping_mul(2_654_435_761);
    for _ in 0..3 {
        out.push(CONSONANTS[k as usize % CONSONANTS.len()]);
        k /= CONSONANTS.len() as u32;
        out.push(POINTS[k as usize % POINTS.len()]);
        k = k.wrapping_mul(31).wrapping_add(7);
    }
    out
}

fn gloss(n: u32) -> &'static str {
    GLOSSES[n as usize % GLOSSES.len()]
}

/// A Hebrew Torah (Genesis to Deuteronomy, about TAHOT's size: 10–30 words a verse) whose
/// every word is Tagged with a Strong's number, Lemma, Morphology and Gloss.
pub(crate) fn hebrew_bible() -> Option<Resource> {
    let mut m = Manifest::new(
        "bench-hot",
        "Bench Hebrew Torah",
        "BHOT",
        ResourceType::Bible,
        "hbo",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let mut rng = Rng(0x4eb_7e57);
    for book in TORAH {
        let unit = UnitId::book(book);
        let mut text = String::new();
        let mut verses = Vec::new();
        let mut tagged = Vec::new();
        let mut chapters = Vec::new();
        for chapter in 1..=Standard.chapter_count(book) {
            let last = Standard.last_verse(book, chapter).unwrap_or(1);
            let chapter_start = text.len();
            for v in 1..=last {
                if !text.is_empty() {
                    text.push(' ');
                }
                let start = text.len();
                for i in 0..10 + rng.below(21) {
                    if i > 0 {
                        text.push(' ');
                    }
                    // Common words are common: the low numbers come up most.
                    let n = 1 + u32::try_from(rng.below(u64::from(LEMMAS)) * rng.below(100) / 100)
                        .unwrap_or(0);
                    let at = text.len();
                    let prefixed = rng.below(4) == 0;
                    if prefixed {
                        text.push_str("וְ");
                    }
                    text.push_str(&lemma(n));
                    let Ok(strongs) = format!("H{n}").parse() else {
                        continue;
                    };
                    let mut part = MorphemePart::new(strongs);
                    part.lemma = Some(lemma(n));
                    part.morph = Some((*rng.pick(MORPHS)).to_string());
                    part.gloss = Some(gloss(n).to_string());
                    let mut parts = Vec::new();
                    if prefixed && let Ok(and) = "H9001".parse() {
                        let mut conj = MorphemePart::new(and);
                        conj.morph = Some("HC".into());
                        conj.gloss = Some("and".into());
                        parts.push(conj);
                    }
                    parts.push(part);
                    tagged.push((at..text.len(), parts));
                }
                text.push('׃');
                verses.push((start..text.len(), Verse::new(book, chapter, v)));
            }
            chapters.push((chapter, chapter_start..text.len()));
            text.push('\n');
        }
        b.add_unit(
            unit.clone(),
            UnitKind::BibleBook,
            Some(book.name().into()),
            text,
        );
        for (chapter, r) in chapters {
            b.add_span(&unit, r.clone(), SpanKind::Chapter { chapter });
            b.add_span(
                &unit,
                r,
                SpanKind::Para {
                    style: ParaStyle::Prose,
                },
            );
        }
        for (r, parts) in tagged {
            b.add_span(&unit, r, SpanKind::TaggedWord { parts });
        }
        for (r, v) in verses {
            b.add_verse(&unit, r, v, v.verse);
        }
    }
    b.build().ok()
}

/// How many Tagged words a chapter of a Bible Resource has: the Exegetical Guide's Word by
/// word count for it.
pub(crate) fn tagged_words(bible: &Resource, book: Book, chapter: u16) -> usize {
    let unit = UnitId::book(book);
    let verses: Vec<_> = bible
        .verses
        .iter()
        .filter(|v| v.unit == unit && v.verse.chapter == chapter)
        .collect();
    let (Some(first), Some(last)) = (verses.first(), verses.last()) else {
        return 0;
    };
    bible
        .units
        .iter()
        .filter(|u| u.id == unit)
        .flat_map(|u| &u.spans)
        .filter(|s| {
            matches!(s.kind, SpanKind::TaggedWord { .. })
                && s.start >= first.start
                && s.end <= last.end
        })
        .count()
}

/// A Hebrew Lexicon with an entry for every Strong's number of [`hebrew_bible`].
pub(crate) fn lexicon() -> Option<Resource> {
    let mut m = Manifest::new(
        "bench-lexicon",
        "Bench Hebrew Lexicon",
        "BLEX",
        ResourceType::Lexicon,
        "en",
    );
    m.key_schemes = vec![KeyScheme::StrongsNumber];
    m.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(m);
    for n in 1..=LEMMAS {
        let Ok(strongs) = format!("H{n}").parse() else {
            continue;
        };
        let Ok(unit) = UnitId::new(format!("H{n}")) else {
            continue;
        };
        let head = lemma(n);
        b.add_unit(
            unit.clone(),
            UnitKind::Entry,
            Some(head.clone()),
            format!(
                "{head}\n{}: a word of the Torah, used in many senses. It is found in narrative \
                 and law alike, and its forms follow the common patterns of its stem.",
                gloss(n)
            ),
        );
        b.add_key(Key::StrongsNumber(strongs), &unit, None, false);
    }
    b.build().ok()
}

/// A People & places index under the id the Atlas reads Place locations from
/// (`openbible-geo`): [`PLACES`] places around the Levant with one to three locations each,
/// and the chapters of [`TORAH`] naming 8–37 of them ([`BUSY_CHAPTER`]: 120).
pub(crate) fn places() -> Option<Resource> {
    let mut m = Manifest::new(
        "openbible-geo",
        "Bench places",
        "BGEO",
        ResourceType::PeopleAndPlaces,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "CC-BY-4.0".into();
    let mut b = ResourceBuilder::new(m);
    let mut rng = Rng(0x9e0_9e0);
    let mut ids = Vec::with_capacity(PLACES);
    for n in 0..PLACES {
        let syllables = 2 + rng.below(2);
        let name = capitalized(&made_up_word(&mut rng, syllables));
        let id = format!("{name}@P{n}");
        let mut locations = Vec::new();
        for k in 0..1 + rng.below(3) {
            // Egypt to Mesopotamia: 29–45 °E, 28–37 °N.
            let lon = 29_000_000 + i32::try_from(rng.below(16_000_000)).unwrap_or(0);
            let lat = 28_000_000 + i32::try_from(rng.below(9_000_000)).unwrap_or(0);
            let score = if k == 0 {
                [800, 300, 60][usize::try_from(rng.below(3)).unwrap_or(0)]
            } else {
                40
            };
            locations.push(PlaceLocation {
                name: format!("{name} site {}", k + 1),
                lon_e6: lon,
                lat_e6: lat,
                score,
            });
        }
        b.add_entity(Entity {
            id: id.clone(),
            name,
            kind: EntityKind::Place,
            strongs: None,
            other_strongs: Vec::new(),
            other_names: Vec::new(),
            region: Some("Bench region".into()),
            relations: Vec::new(),
            locations,
        });
        ids.push(id);
    }
    for book in TORAH {
        for chapter in 1..=Standard.chapter_count(book) {
            let last = Standard.last_verse(book, chapter).unwrap_or(1);
            let count = if chapter == BUSY_CHAPTER {
                BUSY_CHAPTER_PLACES
            } else {
                8 + usize::try_from(rng.below(30)).unwrap_or(0)
            };
            let first = usize::try_from(rng.below(PLACES as u64)).unwrap_or(0);
            for k in 0..count {
                let id = &ids[(first + k * 7) % PLACES];
                let verse = 1 + u16::try_from(rng.below(u64::from(last))).unwrap_or(0);
                b.add_mention(id, Passage::verse(Verse::new(book, chapter, verse)));
            }
        }
    }
    b.build().ok()
}

/// One line of a topical index entry: `marker label ref; ref; …`, with a Scripture reference
/// span on each reference.
fn topic_line(
    text: &mut String,
    spans: &mut Vec<(std::ops::Range<usize>, SpanKind)>,
    marker: &str,
    label: &str,
    passages: &[Passage],
) {
    text.push_str(marker);
    text.push_str(label);
    for (i, p) in passages.iter().enumerate() {
        text.push_str(if i == 0 { " " } else { "; " });
        let start = text.len();
        text.push_str(&format_passage(*p, &Standard, RefStyle::Short));
        spans.push((
            start..text.len(),
            SpanKind::ScriptureRef {
                targets: vec![*p].into(),
            },
        ));
    }
    text.push('\n');
}

fn random_verse(rng: &mut Rng) -> Passage {
    let book = *rng.pick(&Book::ALL);
    let chapter =
        1 + u16::try_from(rng.below(u64::from(Standard.chapter_count(book)))).unwrap_or(0);
    let last = Standard.last_verse(book, chapter).unwrap_or(1);
    let verse = 1 + u16::try_from(rng.below(u64::from(last))).unwrap_or(0);
    Passage::verse(Verse::new(book, chapter, verse))
}

/// A topical index under an id the Topic Guide knows (`nave` laid out like Nave's, with `→`
/// subtopics; `torrey` like Torrey's, with `- `): [`TOPICS`] short topics, the
/// [`LONG_TOPICS`], and see-only entries that send the reader to them.
pub(crate) fn topical_index(id: &str) -> Option<Resource> {
    let (title, abbreviation, marker, seed) = match id {
        "nave" => ("Bench topical Bible (Nave's layout)", "BNave", "→ ", 0x7a5e),
        _ => (
            "Bench topical textbook (Torrey's layout)",
            "BTorrey",
            "- ",
            0x7044,
        ),
    };
    let mut m = Manifest::new(id, title, abbreviation, ResourceType::Dictionary, "en");
    m.key_schemes = vec![KeyScheme::Headword];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let mut rng = Rng(seed);
    let mut topics: Vec<(String, usize, usize)> = LONG_TOPICS
        .iter()
        .map(|(t, s, p)| (t.to_uppercase(), *s, *p))
        .collect();
    for n in 0..TOPICS {
        let syllables = 2 + rng.below(3);
        let name = format!("{}{n}", made_up_word(&mut rng, syllables)).to_uppercase();
        let subtopics = 1 + usize::try_from(rng.below(4)).unwrap_or(0);
        let passages = 1 + usize::try_from(rng.below(6)).unwrap_or(0);
        topics.push((name, subtopics, passages));
    }
    for (n, (topic, subtopics, per)) in topics.iter().enumerate() {
        let Ok(unit) = UnitId::new(format!("t{n}")) else {
            continue;
        };
        let mut text = String::new();
        let mut spans = Vec::new();
        for s in 0..*subtopics {
            let label = format!("{} {s}", made_up_word(&mut rng, 2).to_uppercase());
            let passages: Vec<Passage> = (0..*per).map(|_| random_verse(&mut rng)).collect();
            topic_line(&mut text, &mut spans, marker, &label, &passages);
        }
        b.add_unit(unit.clone(), UnitKind::Entry, Some(topic.clone()), text);
        for (r, kind) in spans {
            b.add_span(&unit, r, kind);
        }
        b.add_key(Key::Headword(topic.clone()), &unit, None, false);
    }
    // Synonyms: entries that only send the reader to a long topic.
    for (n, (topic, _, _)) in LONG_TOPICS.iter().enumerate() {
        for k in 0..3 {
            let head = format!("ASKING {n}{k}");
            let Ok(unit) = UnitId::new(format!("see{n}-{k}")) else {
                continue;
            };
            b.add_unit(
                unit.clone(),
                UnitKind::Entry,
                Some(head.clone()),
                format!("See {}\n", topic.to_uppercase()),
            );
            b.add_key(Key::Headword(head), &unit, None, false);
        }
    }
    b.build().ok()
}

/// A Dictionary with an article on each long topic, as the Topic Guide's Dictionary articles
/// section shows.
pub(crate) fn dictionary() -> Option<Resource> {
    let mut m = Manifest::new(
        "bench-dictionary",
        "Bench Bible Dictionary",
        "BDICT",
        ResourceType::Dictionary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Headword];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let mut rng = Rng(0xd1c7);
    for (n, (topic, _, _)) in LONG_TOPICS.iter().enumerate() {
        let Ok(unit) = UnitId::new(format!("a{n}")) else {
            continue;
        };
        let body: Vec<String> = (0..400).map(|_| (*rng.pick(WORDS)).to_string()).collect();
        let text = format!("{topic}\n{}.", body.join(" "));
        b.add_unit(unit.clone(), UnitKind::Entry, Some((*topic).into()), text);
        b.add_span(&unit, 0..topic.len(), SpanKind::Heading { level: 2 });
        b.add_key(Key::Headword((*topic).into()), &unit, None, false);
    }
    b.build().ok()
}

/// A Get Resources catalog of [`CATALOG_ROWS`] modules from four repositories: mostly Bibles in
/// 60 languages, with Commentaries, Dictionaries and Lexicons, under the licences SWORD confs
/// declare.
pub(crate) fn catalog(fetched_at: u64) -> ResourceCatalog {
    const REPOS: [&str; 4] = ["CrossWire", "eBible.org", "IBT", "CrossWire Attic"];
    const LICENCES: [&str; 6] = [
        "Public Domain",
        "Creative Commons: BY-SA 4.0",
        "Creative Commons: BY-NC-ND 4.0",
        "Copyrighted; Free non-commercial distribution",
        "Copyrighted; Permission to distribute granted to CrossWire",
        "GPL",
    ];
    let mut rng = Rng(0xca7a);
    let languages: Vec<String> = (0..60)
        .map(|n| match n {
            0 => "en".to_string(),
            1 => "grc".into(),
            2 => "hbo".into(),
            _ => format!("x{}", made_up_word(&mut rng, 1)),
        })
        .collect();
    let repos = REPOS
        .iter()
        .map(|r| RepoRef {
            name: (*r).into(),
            uid: None,
            urls: vec![format!("https://example.invalid/{r}")],
        })
        .collect();
    let listings = (0..CATALOG_ROWS)
        .map(|n| {
            let kind = match rng.below(10) {
                0 => ResourceType::Commentary,
                1 => ResourceType::Dictionary,
                2 => ResourceType::Lexicon,
                _ => ResourceType::Bible,
            };
            let name = capitalized(&made_up_word(&mut rng, 3));
            let language = if rng.below(3) == 0 {
                languages[0].clone()
            } else {
                rng.pick(&languages).clone()
            };
            ResourceListing {
                module: format!("{name}{n}"),
                kind,
                repo: (*rng.pick(&REPOS)).into(),
                title: format!("{name} {} ({language})", type_title(kind)),
                abbreviation: format!("{}{n}", name.to_uppercase()),
                language,
                size: Some(1_000_000 + rng.below(9_000_000)),
                licence: Some((*rng.pick(&LICENCES)).into()),
                version: Some(format!("1.{}", rng.below(9))),
                strongs: kind == ResourceType::Bible && rng.below(8) == 0,
                morphology: kind == ResourceType::Bible && rng.below(16) == 0,
                refused: None,
                source: ListingSource::Sword,
                evidence_url: None,
                declared_licence_class: None,
            }
        })
        .collect();
    ResourceCatalog::new(fetched_at, repos, listings)
}

fn type_title(kind: ResourceType) -> &'static str {
    match kind {
        ResourceType::Commentary => "Commentary",
        ResourceType::Dictionary => "Dictionary",
        ResourceType::Lexicon => "Lexicon",
        _ => "Bible",
    }
}

/// Write each Resource to `dir/<id>.obr`.
pub(crate) fn write_library(dir: &Path, resources: &[Resource]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for r in resources {
        let path = dir.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}
