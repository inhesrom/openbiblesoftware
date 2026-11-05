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
        let mut m = Manifest::new(
            id,
            format!("{abbrev} (test)"),
            abbrev,
            ResourceType::Bible,
            "en",
        );
        m.key_schemes = vec![KeyScheme::Passage];
        m.direction = Direction::Ltr;
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
            for (verse, range) in placed {
                b.add_span(
                    &unit,
                    range.clone(),
                    SpanKind::Para {
                        style: ParaStyle::Prose,
                    },
                );
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
