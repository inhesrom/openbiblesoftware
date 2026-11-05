//! Mock SWORD repositories for tests (feature `test-support`): a master list and several
//! `file://` repositories built from the open-text test modules. No network, nothing
//! copyrighted; "NASB1995" here is the public-domain KJV excerpt under another name.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use obs_formats::sword_repo::{Fetch, FetchError, LocalFetch, MASTER_REPO_LIST_URL, Progress};

/// The open-text SWORD test modules.
pub fn sword_assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets/sword")
}

/// A conf from the test assets under a new module name, with some keys replaced.
fn conf_variant(asset: &str, name: &str, overrides: &[(&str, &str)]) -> String {
    let text = std::fs::read_to_string(sword_assets().join("mods.d").join(asset)).unwrap();
    let mut out = format!("[{name}]\n");
    for line in text.lines().skip(1) {
        let key = line.split('=').next().unwrap_or_default();
        if overrides.iter().any(|(k, _)| *k == key) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    for (k, v) in overrides {
        out.push_str(&format!("{k}={v}\n"));
    }
    out
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

/// Repositories on the mock master list:
/// - "Lockman Foundation": `NASB1995` (copyrighted licence line, `InstallSize`).
/// - "CrossWire": `ObsKjvOsis`, `ObsKjvGbf`, `ObsHebrew`, a locked `ObsLocked`, and the
///   commentary `ObsNotes` (not a Bible).
/// - "eBible.org": a second `ObsKjvOsis` and `ObsGreekBible` (`Lang=grc`).
/// - "Broken": a directory that does not exist.
#[derive(Debug)]
pub struct MockRepos {
    pub root: PathBuf,
    master: String,
    /// While set, every fetch fails as if there were no network.
    pub offline: AtomicBool,
    /// URLs fetched, in order.
    pub fetched: Mutex<Vec<String>>,
}

impl MockRepos {
    pub fn new(root: &Path) -> Self {
        let _ = std::fs::remove_dir_all(root);
        let repo = |name: &str, confs: &[(&str, String)]| {
            let dir = root.join(name);
            std::fs::create_dir_all(dir.join("mods.d")).unwrap();
            copy_dir(&sword_assets().join("modules"), &dir.join("modules"));
            for (file, text) in confs {
                std::fs::write(dir.join("mods.d").join(file), text).unwrap();
            }
            dir
        };
        let plain = |asset: &str| {
            std::fs::read_to_string(sword_assets().join("mods.d").join(asset)).unwrap()
        };
        let lockman = repo(
            "lockman",
            &[(
                "nasb1995.conf",
                conf_variant(
                    "obskjvosis.conf",
                    "NASB1995",
                    &[
                        ("Description", "New American Standard Bible - NASB 1995"),
                        ("Abbreviation", "NASB95"),
                        (
                            "DistributionLicense",
                            "Copyrighted; Permission to distribute granted to CrossWire",
                        ),
                    ],
                ),
            )],
        );
        let crosswire = repo(
            "crosswire",
            &[
                ("obskjvosis.conf", plain("obskjvosis.conf")),
                ("obskjvgbf.conf", plain("obskjvgbf.conf")),
                ("obshebrew.conf", plain("obshebrew.conf")),
                ("obsnotes.conf", plain("obsnotes.conf")),
                (
                    "obslocked.conf",
                    conf_variant("obskjvgbf.conf", "ObsLocked", &[("CipherKey", "")]),
                ),
            ],
        );
        let ebible = repo(
            "ebible",
            &[
                (
                    "obskjvosis.conf",
                    conf_variant(
                        "obskjvosis.conf",
                        "ObsKjvOsis",
                        &[("Description", "eBible copy")],
                    ),
                ),
                (
                    "obsgreekbible.conf",
                    conf_variant(
                        "obskjvgbf.conf",
                        "ObsGreekBible",
                        &[("Lang", "grc"), ("Description", "Greek test Bible")],
                    ),
                ),
            ],
        );
        // Forward slashes, so Windows paths make `file:///C:/…` URLs too.
        let dir = |p: &Path| p.display().to_string().replace('\\', "/");
        let master = format!(
            "[Repos]\n1=DIRSource=Lockman Foundation||{}\n2=DIRSource=CrossWire||{}\n3=DIRSource=eBible.org||{}\n4=DIRSource=Broken||{}\n",
            dir(&lockman),
            dir(&crosswire),
            dir(&ebible),
            dir(&root.join("missing")),
        );
        Self {
            root: root.to_path_buf(),
            master,
            offline: AtomicBool::new(false),
            fetched: Mutex::new(Vec::new()),
        }
    }

    pub fn set_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::Relaxed);
    }
}

/// The size the mock Lockman repository reports for its NASB1995 package zip (HEAD only;
/// downloads fall back to the raw module files, as from a repository without packages).
pub const NASB_PACKAGE_SIZE: u64 = 4_472_092;

impl Fetch for MockRepos {
    fn size(&self, url: &str) -> Result<Option<u64>, FetchError> {
        self.fetched.lock().unwrap().push(format!("HEAD {url}"));
        if self.offline.load(Ordering::Relaxed) {
            return Err(FetchError::Network(format!("{url}: offline")));
        }
        if url.ends_with("/lockman/packages/NASB1995.zip") {
            return Ok(Some(NASB_PACKAGE_SIZE));
        }
        LocalFetch.size(url)
    }

    fn get(&self, url: &str, progress: Progress<'_>) -> Result<Vec<u8>, FetchError> {
        self.fetched.lock().unwrap().push(url.to_string());
        if self.offline.load(Ordering::Relaxed) {
            return Err(FetchError::Network(format!(
                "{url}: failed to lookup address information"
            )));
        }
        if url == MASTER_REPO_LIST_URL {
            let bytes = self.master.as_bytes().to_vec();
            let n = bytes.len() as u64;
            return if progress(n, Some(n)) {
                Ok(bytes)
            } else {
                Err(FetchError::Cancelled)
            };
        }
        LocalFetch.get(url, progress)
    }
}

/// One section of [`commentary`]: its Passage, an optional heading the text starts with,
/// and the prose after it.
pub type TestSection<'a> = (obs_scripture::Passage, Option<&'a str>, &'a str);

/// A small public-domain-style Commentary (synthetic prose, nothing quoted from a Starter
/// edition) with Passage-keyed sections, as the Starter converters build them.
pub fn commentary(
    id: &str,
    title: &str,
    abbreviation: &str,
    sections: &[TestSection<'_>],
) -> obs_model::Resource {
    use obs_model::{
        Key, KeyScheme, Manifest, ParaStyle, ResourceBuilder, ResourceType, SpanKind, UnitId,
        UnitKind,
    };
    let mut manifest = Manifest::new(id, title, abbreviation, ResourceType::Commentary, "en");
    manifest.key_schemes = vec![KeyScheme::Passage];
    manifest.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(manifest);
    for (passage, heading, prose) in sections {
        let unit = UnitId::new(format!(
            "{}-{}",
            passage.start().osis(),
            passage.end().osis()
        ))
        .unwrap();
        let mut text = String::new();
        if let Some(h) = heading {
            text.push_str(h);
            text.push('\n');
        }
        let body = text.len();
        text.push_str(prose);
        let len = text.len();
        b.add_unit(
            unit.clone(),
            UnitKind::Section,
            Some(passage.to_string()),
            text,
        );
        if heading.is_some() {
            b.add_span(&unit, 0..body - 1, SpanKind::Heading { level: 2 });
        }
        b.add_span(
            &unit,
            body..len,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
        b.add_key(Key::Passage(*passage), &unit, None, false);
    }
    b.build().unwrap()
}

/// Open-text Bibles for Search Tool tests (#59): hand-made KJV/WEB excerpts, a long Psalm,
/// and a TAHOT excerpt imported from the STEPBible test asset plus a TAHOT-style Gen 1:27.
pub mod bibles {
    use std::path::Path;

    use obs_model::{
        Direction, KeyScheme, Manifest, MorphemePart, Origin, ParaStyle, Resource, ResourceBuilder,
        ResourceType, SpanKind, TextAccumulator, UnitId, UnitKind,
    };
    use obs_scripture::{Book, Passage, Verse, VerseMap, VerseMapEntry};

    pub fn v(book: Book, chapter: u16, verse: u16) -> Verse {
        Verse::new(book, chapter, verse)
    }

    /// A Bible of `verses` (canonical order), one unit per book, each verse its own paragraph.
    /// `standard` maps each verse to its Standard Passage (`None`: Unmapped); without it the
    /// Bible numbers like the Standard. `tagged` words get one Strong's number each.
    pub fn bible(
        id: &str,
        abbrev: &str,
        verses: &[(Verse, &str)],
        standard: Option<&[Option<Verse>]>,
        tagged: &[(&str, &str)],
    ) -> Resource {
        build_bible(id, abbrev, verses, standard, tagged, Direction::Ltr, false)
    }

    /// A Bible of `verses` printed as a real paragraphed Bible is (the WEB, BSB, TAHOT): each
    /// book one paragraph, so most verses (and their numbers) start mid-line.
    pub fn paragraphed_bible(
        id: &str,
        abbrev: &str,
        verses: &[(Verse, &str)],
        direction: Direction,
    ) -> Resource {
        build_bible(id, abbrev, verses, None, &[], direction, true)
    }

    fn build_bible(
        id: &str,
        abbrev: &str,
        verses: &[(Verse, &str)],
        standard: Option<&[Option<Verse>]>,
        tagged: &[(&str, &str)],
        direction: Direction,
        one_paragraph: bool,
    ) -> Resource {
        let mut m = Manifest::new(
            id,
            format!("{abbrev} (test)"),
            abbrev,
            ResourceType::Bible,
            "en",
        );
        m.key_schemes = vec![KeyScheme::Passage];
        m.direction = direction;
        m.license = "LicenseRef-PD".into();
        m.attribution = "Public domain test text.".into();
        m.origin = Origin::Imported;
        m.source_format = "hand-made".into();
        m.import_tool_version = "test".into();
        let mut b = ResourceBuilder::new(m);
        let mut books: Vec<Book> = verses.iter().map(|(v, _)| v.book).collect();
        books.dedup();
        for book in books {
            let unit = UnitId::book(book);
            let mut t = TextAccumulator::new();
            let mut placed = Vec::new();
            for (verse, text) in verses.iter().filter(|(v, _)| v.book == book) {
                if t.pos() > 0 {
                    t.push(" ");
                }
                placed.push((*verse, t.push(text)));
            }
            let text = t.into_string();
            b.add_unit(unit.clone(), UnitKind::BibleBook, None, text.clone());
            if one_paragraph {
                b.add_span(
                    &unit,
                    0..text.len(),
                    SpanKind::Para {
                        style: ParaStyle::Prose,
                    },
                );
            }
            for (verse, range) in placed {
                if !one_paragraph {
                    b.add_span(
                        &unit,
                        range.clone(),
                        SpanKind::Para {
                            style: ParaStyle::Prose,
                        },
                    );
                }
                for (word, strongs) in tagged {
                    if let Some(at) = text[range.clone()].find(word) {
                        let at = range.start + at;
                        b.add_span(
                            &unit,
                            at..at + word.len(),
                            SpanKind::TaggedWord {
                                parts: vec![MorphemePart::new(strongs.parse().unwrap())],
                            },
                        );
                    }
                }
                b.add_verse(&unit, range, verse, verse.verse);
            }
        }
        if let Some(standard) = standard {
            let entries = b
                .verses()
                .iter()
                .zip(standard)
                .map(|(bv, s)| VerseMapEntry {
                    verse: bv.verse,
                    last: bv.last,
                    standard: s.map(Passage::verse),
                })
                .collect();
            b.set_verse_map(VerseMap::new(entries, Vec::new(), &obs_scripture::Standard));
        }
        b.build().unwrap()
    }

    pub const LOVE_KJV: &[(Book, u16, u16, &str)] = &[
        (
            Book::Leviticus,
            19,
            18,
            "Thou shalt not avenge, nor bear any grudge against the children of thy people, but thou shalt love thy neighbour as thyself: I am the LORD.",
        ),
        (
            Book::Matthew,
            5,
            43,
            "Ye have heard that it hath been said, Thou shalt love thy neighbour, and hate thine enemy.",
        ),
        (
            Book::Matthew,
            22,
            39,
            "And the second is like unto it, Thou shalt love thy neighbour as thyself.",
        ),
        (
            Book::Romans,
            13,
            10,
            "Love worketh no ill to his neighbour: therefore love is the fulfilling of the law.",
        ),
    ];

    pub const LOVE_WEB: &[(Book, u16, u16, &str)] = &[
        (
            Book::Leviticus,
            19,
            18,
            "“You shall not take vengeance, nor bear any grudge against the children of your people; but you shall love your neighbor as yourself. I am Yahweh.",
        ),
        (
            Book::Matthew,
            5,
            43,
            "“You have heard that it was said, ‘You shall love your neighbor, and hate your enemy.’",
        ),
        (
            Book::Matthew,
            22,
            39,
            "A second likewise is this, ‘You shall love your neighbor as yourself.’",
        ),
        (
            Book::Romans,
            13,
            10,
            "Love doesn’t harm a neighbor. Love therefore is the fulfillment of the law.",
        ),
    ];

    pub fn verses(list: &[(Book, u16, u16, &'static str)]) -> Vec<(Verse, &'static str)> {
        list.iter().map(|&(b, c, n, t)| (v(b, c, n), t)).collect()
    }

    /// The TAHOT Gen 1:1 lines of the STEPBible test excerpt, plus a TAHOT-style Gen 1:27 whose
    /// first word is וַ/יִּבְרָא (prefix H9001, root H1254A).
    pub fn tahot() -> Resource {
        let asset = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../formats/tests/assets/stepbible/tahot-gen-deu.txt");
        let text = std::fs::read_to_string(asset).unwrap();
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| l.starts_with("Gen.1.1#"))
            .map(String::from)
            .collect();
        assert_eq!(lines.len(), 7, "Gen 1:1 has seven words");
        let row = |cols: [&str; 12]| cols.join("\t") + "\t\t\t\t";
        lines.push(row([
            "Gen.1.27#01=L",
            "וַ/יִּבְרָ֨א",
            "vai./yiv.Ra'",
            "and/ he created",
            "H9001/{H1254A}",
            "HC/Vqw3ms",
            "",
            "",
            "H1254A",
            "",
            "",
            "H9001=ו=and/{H1254A=בָּרָא=to create}",
        ]));
        lines.push(row([
            "Gen.1.27#02=L",
            "אֱלֹהִ֤ים",
            "'E.lo.Him",
            "God",
            "{H0430G}",
            "HNcmpa",
            "",
            "",
            "H0430G",
            "",
            "",
            "{H0430G=אֱלֹהִים=God}",
        ]));
        lines.push(row([
            "Gen.1.27#03=L",
            "אֶת\\־",
            "'et-",
            "<obj.>",
            "{H0853}",
            "HTo",
            "",
            "",
            "H0853",
            "",
            "",
            "{H0853=אֵת=[Obj.]}",
        ]));
        lines.push(row([
            "Gen.1.27#04=L",
            "הָֽ/אָדָם֙\\׃",
            "ha./'a.Dam",
            "the/ man",
            "H9009/{H0120G}\\H9016",
            "HTd/Ncmsa",
            "",
            "",
            "H0120G",
            "",
            "",
            "H9009=ה=the/{H0120G=אָדָם=man}\\H9016=׃=verseEnd",
        ]));
        let mut import = obs_formats::StepbibleImport::new(obs_formats::StepbibleOptions {
            text: obs_formats::StepbibleText::Tahot,
            resource_id: "tahot".into(),
            title: "TAHOT (test)".into(),
            abbreviation: "TAHOT".into(),
            source_file: "tahot".into(),
            tool_version: "test".into(),
            ..Default::default()
        });
        import.add_file("tahot-test.txt", lines.join("\n"));
        import.finish().unwrap().0
    }

    /// The KJV and WEB of four "love thy neighbour" verses (KJV `love` and `neighbour` tagged
    /// H157 and H7453).
    pub fn kjv() -> Resource {
        bible(
            "kjv",
            "KJV",
            &verses(LOVE_KJV),
            None,
            &[("love", "H157"), ("neighbour", "H7453")],
        )
    }

    pub fn web() -> Resource {
        bible("web", "WEB", &verses(LOVE_WEB), None, &[])
    }

    /// Psalm 119:1-`n`, each verse mentioning "love" and "law".
    pub fn psalms(n: u16) -> Resource {
        let text: Vec<(Verse, String)> = (1..=n)
            .map(|k| {
                (
                    v(Book::Psalms, 119, k),
                    format!("Verse {k}: O how love I thy law! it is my meditation all the day."),
                )
            })
            .collect();
        let refs: Vec<(Verse, &str)> = text.iter().map(|(v, t)| (*v, t.as_str())).collect();
        bible("psalms", "PSA", &refs, None, &[])
    }

    /// Write each Resource to `dir/<id>.obr`.
    pub fn write_library(dir: &Path, resources: &[Resource]) {
        std::fs::create_dir_all(dir).unwrap();
        for r in resources {
            obs_store::write_obr(r, &dir.join(format!("{}.obr", r.manifest.resource_id))).unwrap();
        }
    }
}

/// A small Strong's-tagged Bible (one unit per Bible book, one paragraph per chapter) for
/// Guide tests (#54). Each verse is written as words; a tagged word carries its parts in
/// braces: `loved{G25}`, `loved{G25|ἀγαπάω|V-AAI-3S|loved}`, several parts joined by `+`.
/// `_` inside a word or a part is a space (`only_begotten{G3439}`).
pub fn tagged_bible(
    id: &str,
    title: &str,
    abbreviation: &str,
    language: &str,
    verses: &[(obs_scripture::Verse, &str)],
) -> obs_model::Resource {
    use obs_model::{
        KeyScheme, Manifest, MorphemePart, ParaStyle, ResourceBuilder, ResourceType, SpanKind,
        UnitId, UnitKind,
    };
    let mut manifest = Manifest::new(id, title, abbreviation, ResourceType::Bible, language);
    manifest.key_schemes = vec![KeyScheme::Passage];
    manifest.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(manifest);
    let mut books: Vec<obs_scripture::Book> = verses.iter().map(|(v, _)| v.book).collect();
    books.dedup();
    for book in books {
        let unit = UnitId::book(book);
        let mut text = String::new();
        let mut spans: Vec<(std::ops::Range<usize>, SpanKind)> = Vec::new();
        let mut verse_ranges = Vec::new();
        let mut chapters: Vec<(u16, usize, usize)> = Vec::new();
        for (verse, words) in verses.iter().filter(|(v, _)| v.book == book) {
            if !text.is_empty() {
                text.push(' ');
            }
            let start = text.len();
            for (i, token) in words.split_whitespace().enumerate() {
                if i > 0 {
                    text.push(' ');
                }
                // `word{parts}` and any punctuation after the brace.
                let (word, tags, trailing) = match (token.find('{'), token.rfind('}')) {
                    (Some(open), Some(close)) => (
                        &token[..open],
                        Some(&token[open + 1..close]),
                        &token[close + 1..],
                    ),
                    _ => (token, None, ""),
                };
                let at = text.len();
                text.push_str(&word.replace('_', " "));
                let end = text.len();
                if let Some(t) = tags {
                    let parts = t
                        .split('+')
                        .map(|p| {
                            let mut f = p.split('|');
                            let strongs = f.next().unwrap().parse().unwrap();
                            let mut next = || {
                                f.next()
                                    .filter(|s| !s.is_empty())
                                    .map(|s| s.replace('_', " "))
                            };
                            MorphemePart {
                                strongs,
                                lemma: next(),
                                morph: next(),
                                gloss: next(),
                            }
                        })
                        .collect();
                    spans.push((at..end, SpanKind::TaggedWord { parts }));
                }
                text.push_str(trailing);
            }
            let end = text.len();
            verse_ranges.push((start..end, *verse));
            match chapters.last_mut() {
                Some((c, _, e)) if *c == verse.chapter => *e = end,
                _ => chapters.push((verse.chapter, start, end)),
            }
        }
        b.add_unit(
            unit.clone(),
            UnitKind::BibleBook,
            Some(format!("{book:?}")),
            text,
        );
        for (chapter, start, end) in chapters {
            b.add_span(&unit, start..end, SpanKind::Chapter { chapter });
            b.add_span(
                &unit,
                start..end,
                SpanKind::Para {
                    style: ParaStyle::Prose,
                },
            );
        }
        for (range, kind) in spans {
            b.add_span(&unit, range, kind);
        }
        for (range, verse) in verse_ranges {
            b.add_verse(&unit, range, verse, verse.verse);
        }
    }
    b.build().unwrap()
}

/// The TIPNR excerpt of the format tests (STEPBible, CC BY) as a People & places index.
pub fn tipnr() -> obs_model::Resource {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../formats/tests/assets/tipnr/tipnr-excerpt.txt"),
    )
    .unwrap();
    let options = obs_formats::DatasetOptions {
        resource_id: "tipnr".into(),
        title: "Translators Individualised Proper Names".into(),
        ..Default::default()
    };
    obs_formats::import_tipnr(&text, &options).unwrap().0
}

/// Easton's or Smith's Bible Dictionary, reduced to the public-domain Abraham and Abram
/// entries of the #44 study fixtures (with the converter's Abram → Abraham alias).
pub fn dictionary(id: &str) -> obs_model::Resource {
    use obs_formats::{DictionaryData, StudyImport, import_dictionary};
    use obs_model::{Manifest, ResourceType};
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/fixtures/study");
    let bytes = |name: &str| std::fs::read(dir.join(name)).unwrap();
    let (title, abbreviation) = match id {
        "easton" => ("Easton's Bible Dictionary", "Easton"),
        _ => ("Smith's Bible Dictionary", "Smith"),
    };
    let mut manifest = Manifest::new(id, title, abbreviation, ResourceType::Dictionary, "en");
    manifest.license = "LicenseRef-PD".into();
    let mut import = StudyImport::new(manifest);
    let (index, data) = (bytes(&format!("{id}.idx")), bytes(&format!("{id}.dat")));
    let (block_index, blocks) = if id == "easton" {
        (bytes("easton.zdx"), bytes("easton.zdt"))
    } else {
        Default::default()
    };
    let input = if id == "easton" {
        DictionaryData::Easton {
            index: &index,
            data: &data,
            block_index: &block_index,
            blocks: &blocks,
        }
    } else {
        DictionaryData::Smith {
            index: &index,
            data: &data,
        }
    };
    import_dictionary(&mut import, input).unwrap();
    import.finish().unwrap().0
}

/// A Lexicon of synthetic entries, `(Strong's number, Lemma, Gloss)`, laid out as the
/// Starter converters write them: the Lemma line, then the Gloss.
pub fn lexicon(
    id: &str,
    abbreviation: &str,
    entries: &[(&str, &str, &str)],
) -> obs_model::Resource {
    use obs_model::{Key, KeyScheme, Manifest, ResourceBuilder, ResourceType, UnitId, UnitKind};
    let mut manifest = Manifest::new(id, id, abbreviation, ResourceType::Lexicon, "en");
    manifest.key_schemes = vec![KeyScheme::StrongsNumber];
    manifest.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(manifest);
    for (number, lemma, gloss) in entries {
        let unit = UnitId::new(*number).unwrap();
        b.add_unit(
            unit.clone(),
            UnitKind::Entry,
            Some((*lemma).into()),
            format!("{lemma}\n{gloss}"),
        );
        b.add_key(
            Key::StrongsNumber(number.parse().unwrap()),
            &unit,
            None,
            false,
        );
    }
    b.build().unwrap()
}

fn jn(c: u16, v: u16) -> obs_scripture::Verse {
    obs_scripture::Verse::new(obs_scripture::Book::John, c, v)
}

fn gn12(v: u16) -> obs_scripture::Verse {
    obs_scripture::Verse::new(obs_scripture::Book::Genesis, 12, v)
}

/// KJV (public domain) John 1:1-2 and 3:16-17, tagged with Strong's numbers only, the way
/// KJV+Strong's is: no Lemma, no Morphology, untagged articles.
pub fn kjv_john() -> obs_model::Resource {
    tagged_bible(
        "kjv",
        "King James Version with Strong's",
        "KJV",
        "en",
        &[
            (
                jn(1, 1),
                "In{G1722} the beginning{G746} was{G2258} the Word{G3056}, and{G2532} the \
                 Word{G3056} was{G2258} with{G4314} God{G2316}, and{G2532} the Word{G3056} \
                 was{G2258} God{G2316}.",
            ),
            (
                jn(1, 2),
                "The same{G3778} was{G2258} in{G1722} the beginning{G746} with{G4314} God{G2316}.",
            ),
            (
                jn(3, 16),
                "For{G1063} God{G2316} so{G3779} loved{G25} the world{G2889}, that{G5620} \
                 he_gave{G1325} his{G846} only_begotten{G3439} Son{G5207}, that{G2443} \
                 whosoever{G3956} believeth{G4100} in{G1519} him{G846} should not{G3361} \
                 perish{G622}, but{G235} have{G2192} everlasting{G166} life{G2222}.",
            ),
            (
                jn(3, 17),
                "For{G1063} God{G2316} sent{G649} not{G3756} his{G846} Son{G5207} into{G1519} \
                 the world{G2889} to condemn{G2919} the world{G2889}; but{G235} that{G2443} \
                 the world{G2889} through{G1223} him{G846} might be_saved{G4982}.",
            ),
        ],
    )
}

/// John 3:16 as STEPBible's TAGNT tags it (CC BY): every word with Lemma, Morphology and
/// Gloss, the article included.
pub fn tagnt_john_3_16() -> obs_model::Resource {
    tagged_bible(
        "tagnt",
        "Translators Amalgamated Greek NT",
        "TAGNT",
        "grc",
        &[(
            jn(3, 16),
            "οὕτως{G3779|οὕτω|ADV|Thus} γὰρ{G1063|γάρ|CONJ|for} \
             ἠγάπησεν{G25|ἀγαπάω|V-AAI-3S|loved} ὁ{G3588|ὁ|T-NSM|the} \
             θεὸς{G2316|θεός|N-NSM-T|God} τὸν{G3588|ὁ|T-ASM|the} \
             κόσμον{G2889|κόσμος|N-ASM|world,} ὥστε{G5620|ὥστε|CONJ|that} \
             τὸν{G3588|ὁ|T-ASM|the} υἱὸν{G5207|υἱός|N-ASM|Son} τὸν{G3588|ὁ|T-ASM|the} \
             μονογενῆ{G3439|μονογενής|A-ASM|only_begotten} ἔδωκεν{G1325|δίδωμι|V-AAI-3S|He_gave,} \
             ἵνα{G2443|ἵνα|CONJ|so_that} πᾶς{G3956|πᾶς|A-NSM|everyone} ὁ{G3588|ὁ|T-NSM|who} \
             πιστεύων{G4100|πιστεύω|V-PAP-NSM|is_believing} εἰς{G1519|εἰς|PREP|in} \
             αὐτὸν{G846|αὐτός|P-ASM|Him} μὴ{G3361|μή|PRT-N|not} \
             ἀπόληται{G622|ἀπολλύω|V-2AMS-3S|may_perish} ἀλλ᾽{G235|ἀλλά|CONJ|but} \
             ἔχῃ{G2192|ἔχω|V-PAS-3S|may_have} ζωὴν{G2222|ζωή|N-ASF|life} \
             αἰώνιον{G166|αἰώνιος|A-ASF|eternal.}",
        )],
    )
}

/// KJV Genesis 12:1 (public domain) with Strong's numbers.
pub fn kjv_genesis_12() -> obs_model::Resource {
    tagged_bible(
        "kjv-gen",
        "King James Version (Genesis 12)",
        "KJV",
        "en",
        &[
            (
                gn12(1),
                "Now the LORD{H3068} had said{H559} unto Abram{H87}, Get{H3212} thee out of \
                 thy country{H776}, and from thy kindred{H4138}, and from thy father's{H1} \
                 house{H1004}, unto a land{H776} that I will shew{H7200} thee:",
            ),
            (
                gn12(4),
                "So Abram{H87} departed{H3212}, as the LORD{H3068} had spoken{H1696} unto him; \
                 and Lot{H3876} went{H3212} with him: and Abram{H87} was seventy{H7657} and \
                 five{H2568} years{H8141} old{H1121} when he departed{H3318} out of \
                 Haran{H2771}.",
            ),
        ],
    )
}

/// A Lexicon with G25, G3439 and G3588 (synthetic glosses).
pub fn greek_lexicon() -> obs_model::Resource {
    lexicon(
        "lexicon",
        "LEX",
        &[
            ("G25", "ἀγαπάω", "to love"),
            ("G3439", "μονογενής", "only, unique"),
            ("G3588", "ὁ", "the (definite article)"),
        ],
    )
}

/// One Verse of [`word_tagged_bible`]: the Verse, its text, and its Tagged words in order, each
/// as (the words as written, Strong's number, optional Morphology code). Each word is found
/// in the text after the previous one.
pub type TestVerse<'a> = (
    obs_scripture::Verse,
    &'a str,
    &'a [(&'a str, &'a str, Option<&'a str>)],
);

/// A small Strong's-tagged Bible in the Standard versification, one unit per Bible book.
pub fn word_tagged_bible(
    id: &str,
    title: &str,
    abbreviation: &str,
    language: &str,
    verses: &[TestVerse<'_>],
) -> obs_model::Resource {
    use obs_model::{
        KeyScheme, Manifest, MorphemePart, ParaStyle, ResourceBuilder, ResourceType, SpanKind,
        TextAccumulator, UnitId, UnitKind,
    };
    let mut manifest = Manifest::new(id, title, abbreviation, ResourceType::Bible, language);
    manifest.key_schemes = vec![KeyScheme::Passage];
    manifest.license = "LicenseRef-PD".into();
    if language == "hbo" {
        manifest.direction = obs_model::Direction::Rtl;
    }
    let mut b = ResourceBuilder::new(manifest);
    let mut books: Vec<obs_scripture::Book> = verses.iter().map(|v| v.0.book).collect();
    books.dedup();
    for book in books {
        let unit = UnitId::book(book);
        let mut t = TextAccumulator::new();
        let mut spans = Vec::new();
        let mut placed = Vec::new();
        for (verse, text, words) in verses.iter().filter(|v| v.0.book == book) {
            if t.pos() > 0 {
                t.push(" ");
            }
            let range = t.push(text);
            let mut from = 0;
            for (word, strongs, morph) in *words {
                let at = text[from..].find(word).unwrap() + from;
                from = at + word.len();
                let mut part = MorphemePart::new(strongs.parse().unwrap());
                part.morph = morph.map(String::from);
                spans.push((
                    range.start + at..range.start + from,
                    SpanKind::TaggedWord { parts: vec![part] },
                ));
            }
            placed.push((range, *verse));
        }
        let len = t.pos();
        b.add_unit(unit.clone(), UnitKind::BibleBook, None, t.into_string());
        b.add_span(
            &unit,
            0..len,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
        for (range, kind) in spans {
            b.add_span(&unit, range, kind);
        }
        for (range, verse) in placed {
            b.add_verse(&unit, range, verse, verse.verse);
        }
    }
    b.build().unwrap()
}

/// A KJV-style English Bible with Strong's numbers (KJV text, public domain): "love" renders
/// G25 three times, G26 twice and G5368 twice; "created" renders only H1254.
pub fn kjv_love() -> obs_model::Resource {
    use obs_scripture::{Book, Verse};
    word_tagged_bible(
        "kjv-love",
        "King James Version with Strong's (test)",
        "KJV",
        "en",
        &[
            (
                Verse::new(Book::Genesis, 1, 1),
                "In the beginning God created the heaven and the earth.",
                &[
                    ("God", "H430", None),
                    ("created", "H1254", Some("strongMorph:TH8804")),
                ],
            ),
            (
                Verse::new(Book::Matthew, 6, 5),
                "for they love to pray standing in the synagogues",
                &[("love", "G5368", None), ("pray", "G4336", None)],
            ),
            (
                Verse::new(Book::John, 13, 34),
                "That ye love one another; as I have loved you, that ye also love one another.",
                &[
                    ("love", "G25", None),
                    ("loved", "G25", None),
                    ("love", "G25", None),
                ],
            ),
            (
                Verse::new(Book::John, 14, 15),
                "If ye love me, keep my commandments.",
                &[("love", "G25", None), ("keep", "G5083", None)],
            ),
            (
                Verse::new(Book::John, 15, 13),
                "Greater love hath no man than this",
                &[("love", "G26", None)],
            ),
            (
                Verse::new(Book::John, 21, 15),
                "lovest thou me? Yea, Lord; thou knowest that I love thee.",
                &[("lovest", "G25", None), ("love", "G5368", None)],
            ),
            (
                Verse::new(Book::FirstJohn, 4, 8),
                "He that loveth not knoweth not God; for God is love.",
                &[("loveth", "G25", None), ("love", "G26", None)],
            ),
        ],
    )
}

/// A Hebrew Bible tagged with extended Strong's numbers and readable Morphology, like TAHOT:
/// H1254A three times, always a verb.
pub fn hebrew_bara() -> obs_model::Resource {
    use obs_scripture::{Book, Verse};
    word_tagged_bible(
        "heb-bara",
        "Hebrew Old Testament (test)",
        "HOT",
        "hbo",
        &[
            (
                Verse::new(Book::Genesis, 1, 1),
                "בְּרֵאשִׁית בָּרָא אֱלֹהִים",
                &[
                    ("בָּרָא", "H1254A", Some("HVqp3ms")),
                    ("אֱלֹהִים", "H430G", Some("HNcmpa")),
                ],
            ),
            (
                Verse::new(Book::Genesis, 1, 21),
                "וַיִּבְרָא אֱלֹהִים",
                &[
                    ("וַיִּבְרָא", "H1254A", Some("HVqw3ms")),
                    ("אֱלֹהִים", "H430G", Some("HNcmpa")),
                ],
            ),
            (
                Verse::new(Book::Genesis, 1, 27),
                "וַיִּבְרָא אֱלֹהִים",
                &[
                    ("וַיִּבְרָא", "H1254A", Some("HVqw3ms")),
                    ("אֱלֹהִים", "H430G", Some("HNcmpa")),
                ],
            ),
        ],
    )
}

/// The Starter Lexicons' converters run over the excerpts of their real sources that the
/// format tests use, with the Starter resource ids: TBESH, TBESG, Strong's Hebrew and BDB.
pub fn starter_lexicon_excerpts() -> Vec<obs_model::Resource> {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets/lexicon");
    let read = |name: &str| std::fs::read_to_string(assets.join(name)).unwrap();
    let options = |id: &str, title: &str, abbreviation: &str| obs_formats::LexiconOptions {
        resource_id: id.into(),
        title: title.into(),
        abbreviation: abbreviation.into(),
        source_file: id.into(),
        tool_version: "test".into(),
    };
    let skip = ["Meaning".to_string()];
    vec![
        obs_formats::import_tbes(
            &options(
                "tbesh",
                "Brief lexicon of Extended Strongs for Hebrew",
                "TBESH",
            ),
            &read("tbesh-excerpt.txt"),
            &skip,
        )
        .unwrap()
        .0,
        obs_formats::import_tbes(
            &options(
                "tbesg",
                "Brief lexicon of Extended Strongs for Greek",
                "TBESG",
            ),
            &read("tbesg-excerpt.txt"),
            &[],
        )
        .unwrap()
        .0,
        obs_formats::import_strongs_hebrew(
            &options("strongs-hebrew", "Strong's Hebrew Dictionary", "Strong's"),
            &read("strongs-hebrew-excerpt.xml"),
        )
        .unwrap()
        .0,
        obs_formats::import_bdb(
            &options("bdb", "Brown-Driver-Briggs", "BDB"),
            &read("bdb-excerpt.xml"),
            &read("bdb-lexical-index-excerpt.xml"),
        )
        .unwrap()
        .0,
    ]
}

/// Open-text Commentaries, a Lexicon and a Dictionary for Library search tests (#60):
/// synthetic prose written for these tests (nothing quoted from a Starter edition), several
/// paragraphs per unit, built the way the Starter converters build them (a heading, then
/// `para` spans; one Key per unit).
pub mod study {
    use obs_model::{
        Key, KeyScheme, Manifest, ParaStyle, Resource, ResourceBuilder, ResourceType, SpanKind,
        UnitId, UnitKind,
    };
    use obs_scripture::{Book, Passage, Verse};

    /// One unit: its Key, its title (also its heading) and its paragraphs.
    pub type Entry = (Key, String, Vec<String>);

    /// A Resource of `entries`, one unit each, keyed by their Keys.
    pub fn resource(
        id: &str,
        title: &str,
        abbreviation: &str,
        ty: ResourceType,
        entries: &[Entry],
    ) -> Resource {
        let mut manifest = Manifest::new(id, title, abbreviation, ty, "en");
        manifest.key_schemes = match ty {
            ResourceType::Commentary => vec![KeyScheme::Passage],
            ResourceType::Lexicon => vec![KeyScheme::StrongsNumber],
            _ => vec![KeyScheme::Headword],
        };
        manifest.license = "CC0-1.0".into();
        manifest.attribution = "Synthetic test text.".into();
        let mut b = ResourceBuilder::new(manifest);
        for (key, heading, paragraphs) in entries {
            let unit = match key {
                Key::Passage(p) => UnitId::new(format!("{}-{}", p.start().osis(), p.end().osis())),
                k => UnitId::new(k.to_string()),
            }
            .unwrap();
            let kind = match ty {
                ResourceType::Commentary => UnitKind::Section,
                _ => UnitKind::Entry,
            };
            let mut text = heading.clone();
            let mut paras = Vec::new();
            for p in paragraphs {
                text.push('\n');
                let start = text.len();
                text.push_str(p);
                paras.push(start..text.len());
            }
            b.add_unit(unit.clone(), kind, Some(heading.clone()), text);
            b.add_span(&unit, 0..heading.len(), SpanKind::Heading { level: 2 });
            for r in paras {
                b.add_span(
                    &unit,
                    r,
                    SpanKind::Para {
                        style: ParaStyle::Prose,
                    },
                );
            }
            b.add_key(key.clone(), &unit, None, false);
        }
        b.build().unwrap()
    }

    fn passage(book: Book, c1: u16, v1: u16, c2: u16, v2: u16) -> Key {
        Key::Passage(Passage::new(Verse::new(book, c1, v1), Verse::new(book, c2, v2)).unwrap())
    }

    fn paras(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// A long paragraph without the word "covenant" (`n` tells them apart).
    pub fn filler(n: usize) -> String {
        format!(
            "Remark {n}. The patriarch waits through long years, and the promise of a son is \
             repeated in plain words. He looks at the stars of the night sky and counts what \
             cannot be counted. His servants keep the flocks, his tent is pitched among \
             strangers, and he builds an altar wherever he stops. The narrative moves slowly \
             here so that the reader feels the length of the waiting, and every delay teaches \
             patience. Nothing in this remark is hurried; it is meant to fill the page."
        )
    }

    /// The paragraph of the Gen 15 section that names the covenant: the sixth, after five
    /// long [`filler`] paragraphs.
    pub const GEN15_COVENANT: &str = "Between the pieces a smoking furnace and a burning lamp \
        pass by, and so the LORD makes a covenant with Abram about the land.";

    /// "Test Covenant Commentary" (TCC): Gen 9:8-17, Gen 15 (its covenant paragraph after
    /// five long ones) and Jer 31:31-34.
    pub fn tcc() -> Resource {
        let mut gen15 = paras(&["Abram is afraid, and the word comes to him in a vision."]);
        gen15.extend((1..=5).map(filler));
        gen15.push(GEN15_COVENANT.into());
        resource(
            "tcc",
            "Test Covenant Commentary",
            "TCC",
            ResourceType::Commentary,
            &[
                (
                    passage(Book::Genesis, 9, 8, 9, 17),
                    "Genesis 9:8-17".into(),
                    paras(&[
                        "God speaks to Noah and to his sons once the waters have gone down.",
                        "The covenant with Noah is made with every living creature, and its \
                         sign is the bow in the cloud: a covenant without conditions.",
                    ]),
                ),
                (
                    passage(Book::Genesis, 15, 1, 15, 21),
                    "Genesis 15:1-21".into(),
                    gen15,
                ),
                (
                    passage(Book::Jeremiah, 31, 31, 31, 34),
                    "Jeremiah 31:31-34".into(),
                    paras(&["A new covenant is promised, written on the heart and not on stone."]),
                ),
            ],
        )
    }

    /// "Second Test Commentary" (TC2): Gen 17:1-14.
    pub fn tc2() -> Resource {
        resource(
            "tc2",
            "Second Test Commentary",
            "TC2",
            ResourceType::Commentary,
            &[(
                passage(Book::Genesis, 17, 1, 17, 14),
                "Genesis 17:1-14".into(),
                paras(&[
                    "Abram receives a new name.",
                    "Circumcision is the token of the covenant in the flesh.",
                ]),
            )],
        )
    }

    /// "Test Lexicon" (TLX): H1285 and G1242 mention a covenant, G26 does not.
    pub fn tlx() -> Resource {
        let strongs = |s: &str| Key::StrongsNumber(s.parse().unwrap());
        resource(
            "tlx",
            "Test Lexicon",
            "TLX",
            ResourceType::Lexicon,
            &[
                (
                    strongs("G26"),
                    "ἀγάπη".into(),
                    paras(&["Love, goodwill, esteem."]),
                ),
                (
                    strongs("G1242"),
                    "διαθήκη".into(),
                    paras(&["A covenant; also a last will or testament."]),
                ),
                (
                    strongs("H1285"),
                    "בְּרִית".into(),
                    paras(&["Covenant, alliance, pledge; a treaty between peoples."]),
                ),
            ],
        )
    }

    /// "Test Bible Dictionary" (TBD): Ark, Covenant (three paragraphs) and Rainbow (no
    /// covenant).
    pub fn tbd() -> Resource {
        let headword = |s: &str| Key::Headword(s.into());
        resource(
            "tbd",
            "Test Bible Dictionary",
            "TBD",
            ResourceType::Dictionary,
            &[
                (
                    headword("Ark"),
                    "Ark".into(),
                    paras(&["A chest of acacia wood that held the tables of the covenant."]),
                ),
                (
                    headword("Covenant"),
                    "Covenant".into(),
                    paras(&[
                        "An agreement between two parties, binding on both of them.",
                        "In Scripture a covenant of God with his people is of grace; the old \
                         covenant and the new covenant are compared at length.",
                        "See also Ark and Rainbow.",
                    ]),
                ),
                (
                    headword("Rainbow"),
                    "Rainbow".into(),
                    paras(&["The sign set in the cloud after the flood."]),
                ),
            ],
        )
    }
}

/// KJV verses (public domain) where G5485 is rendered "grace" six times and "favour" three
/// times, tagged with Strong's numbers only, the way KJV+Strong's is (#56).
pub fn kjv_grace() -> obs_model::Resource {
    use obs_scripture::{Book, Verse};
    let v = Verse::new;
    tagged_bible(
        "kjv-grace",
        "King James Version with Strong's (test)",
        "KJV",
        "en",
        &[
            (
                v(Book::Luke, 1, 30),
                "And the angel{G32} said{G2036} unto her, Fear{G5399} not{G3361}, Mary{G3137}: \
                 for thou hast found{G2147} favour{G5485} with God{G2316}.",
            ),
            (
                v(Book::Luke, 2, 40),
                "And the child{G3813} grew{G837}, and waxed_strong{G2901} in spirit{G4151}, \
                 filled{G4137} with wisdom{G4678}: and the grace{G5485} of God{G2316} was{G2258} \
                 upon him.",
            ),
            (
                v(Book::Luke, 2, 52),
                "And Jesus{G2424} increased{G4298} in wisdom{G4678} and stature{G2244}, and in \
                 favour{G5485} with God{G2316} and man{G444}.",
            ),
            (
                v(Book::John, 1, 14),
                "And the Word{G3056} was_made{G1096} flesh{G4561}, and dwelt{G4637} among us, \
                 full{G4134} of grace{G5485} and truth{G225}.",
            ),
            (
                v(Book::John, 1, 16),
                "And of his fulness{G4138} have all{G3956} we received{G2983}, and \
                 grace{G5485} for{G473} grace{G5485}.",
            ),
            (
                v(Book::Acts, 2, 47),
                "Praising{G134} God{G2316}, and having{G2192} favour{G5485} with all{G3650} the \
                 people{G2992}.",
            ),
            (
                v(Book::Romans, 1, 5),
                "By whom we have received{G2983} grace{G5485} and apostleship{G651}, for \
                 obedience{G5218} to the faith{G4102} among all{G3956} nations{G1484}, for his \
                 name{G3686}:",
            ),
            (
                v(Book::Ephesians, 2, 8),
                "For by grace{G5485} are ye saved{G4982} through faith{G4102}; and that not of \
                 yourselves: it is the gift{G1435} of God{G2316}:",
            ),
        ],
    )
}

/// The same Verses as STEPBible's TAGNT tags them (CC BY), with G5485's Morphology:
/// accusative four times, dative twice, genitive twice and nominative once.
pub fn tagnt_grace() -> obs_model::Resource {
    use obs_scripture::{Book, Verse};
    let v = Verse::new;
    let mut r = tagged_bible(
        "tagnt-grace",
        "Translators Amalgamated Greek NT (test)",
        "TAGNT",
        "grc",
        &[
            (
                v(Book::Luke, 1, 30),
                "εὗρες{G2147|εὑρίσκω|V-2AAI-2S|you_have_found} γὰρ{G1063|γάρ|CONJ|for} \
                 χάριν{G5485|χάρις|N-ASF|favor} παρὰ{G3844|παρά|PREP|with} \
                 τῷ{G3588|ὁ|T-DSM|-} θεῷ{G2316|θεός|N-DSM-T|God}.",
            ),
            (
                v(Book::Luke, 2, 40),
                "καὶ{G2532|καί|CONJ|and} χάρις{G5485|χάρις|N-NSF|[the]_grace} \
                 θεοῦ{G2316|θεός|N-GSM-T|of_God} ἦν{G1510|εἰμί|V-IAI-3S|was} \
                 ἐπ᾽{G1909|ἐπί|PREP|upon} αὐτό{G846|αὐτός|P-ASN|Him}.",
            ),
            (
                v(Book::Luke, 2, 52),
                "καὶ{G2532|καί|CONJ|and} χάριτι{G5485|χάρις|N-DSF|in_favor} \
                 παρὰ{G3844|παρά|PREP|with} θεῷ{G2316|θεός|N-DSM-T|God}.",
            ),
            (
                v(Book::John, 1, 14),
                "πλήρης{G4134|πλήρης|A-NSM|full} χάριτος{G5485|χάρις|N-GSF|of_grace} \
                 καὶ{G2532|καί|CONJ|and} ἀληθείας{G225|ἀλήθεια|N-GSF|of_truth}.",
            ),
            (
                v(Book::John, 1, 16),
                "καὶ{G2532|καί|CONJ|and} χάριν{G5485|χάρις|N-ASF|grace} \
                 ἀντὶ{G473|ἀντί|PREP|upon} χάριτος{G5485|χάρις|N-GSF|grace}.",
            ),
            (
                v(Book::Acts, 2, 47),
                "ἔχοντες{G2192|ἔχω|V-PAP-NPM|having} χάριν{G5485|χάρις|N-ASF|favor} \
                 πρὸς{G4314|πρός|PREP|with} ὅλον{G3650|ὅλος|A-ASM|all} \
                 τὸν{G3588|ὁ|T-ASM|the} λαόν{G2992|λαός|N-ASM|people}.",
            ),
            (
                v(Book::Romans, 1, 5),
                "ἐλάβομεν{G2983|λαμβάνω|V-2AAI-1P|we_received} χάριν{G5485|χάρις|N-ASF|grace} \
                 καὶ{G2532|καί|CONJ|and} ἀποστολὴν{G651|ἀποστολή|N-ASF|apostleship}",
            ),
            (
                v(Book::Ephesians, 2, 8),
                "τῇ{G3588|ὁ|T-DSF|-} γὰρ{G1063|γάρ|CONJ|for} χάριτί{G5485|χάρις|N-DSF|by_grace} \
                 ἐστε{G1510|εἰμί|V-PAI-2P|you_are} σεσῳσμένοι{G4982|σῴζω|V-RPP-NPM|saved}",
            ),
        ],
    );
    r.manifest.license = "CC-BY-4.0".into();
    r.manifest.attribution = "STEPBible.org (Tyndale House, Cambridge), CC BY 4.0.".into();
    r
}
