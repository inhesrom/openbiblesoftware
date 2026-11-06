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
/// - "CrossWire": the Bibles `ObsKjvOsis`, `ObsKjvGbf`, `ObsHebrew` and a locked `ObsLocked`;
///   the Lexicon `StrongsGreek` (the open test Greek lexicon under the Featured name, with
///   G26); the Commentary `ObsNotes` (KJV numbering, John 3:16); the Dictionary `ObsTopics`;
///   and two Commentaries Import refuses: `ObsHebrewNotes` (MT numbering) and `ObsLinks`
///   (`HREFCom`); and the Hebrew & Greek Bibles `OSHB` and `MorphGNT` (the tagged test
///   excerpts `ObsHebrewOt` and `ObsGreekNt` under the Featured names, #133).
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
                ("obstopics.conf", plain("obstopics.conf")),
                ("obshebrewnotes.conf", plain("obshebrewnotes.conf")),
                (
                    "strongsgreek.conf",
                    conf_variant(
                        "obsgreek.conf",
                        "StrongsGreek",
                        &[
                            ("Description", "Strong's Greek Bible Dictionary"),
                            ("DistributionLicense", "Public Domain"),
                            ("InstallSize", "863426"),
                        ],
                    ),
                ),
                (
                    "oshb.conf",
                    conf_variant(
                        "obshebrewot.conf",
                        "OSHB",
                        &[
                            ("Description", "Open Scriptures Hebrew Bible"),
                            ("DistributionLicense", "Creative Commons: BY 4.0"),
                            ("InstallSize", "3279140"),
                        ],
                    ),
                ),
                (
                    "morphgnt.conf",
                    conf_variant(
                        "obsgreeknt.conf",
                        "MorphGNT",
                        &[
                            (
                                "Description",
                                "Morphologically Parsed Greek New Testament based on the SBLGNT",
                            ),
                            ("InstallSize", "1388384"),
                        ],
                    ),
                ),
                (
                    "obslinks.conf",
                    "[ObsLinks]\nDataPath=./modules/comments/hrefcom/obslinks/\nModDrv=HREFCom\n\
                     Lang=en\nDescription=Open Bible test web links\n\
                     DistributionLicense=Public Domain\n"
                        .to_string(),
                ),
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

    /// Publish a new version of a module in repository folder `repo` (`crosswire`, …): its
    /// conf's `Version` becomes `version`, and in a RawText module's text every `from`
    /// becomes `to` (the verse index is rewritten to match).
    pub fn publish(&self, repo: &str, conf_file: &str, version: &str, edit: Option<(&str, &str)>) {
        let dir = self.root.join(repo);
        let conf_path = dir.join("mods.d").join(conf_file);
        let conf = std::fs::read_to_string(&conf_path).unwrap();
        let mut out = String::new();
        let mut data_path = None;
        for line in conf.lines() {
            if line.starts_with("Version=") {
                continue;
            }
            if let Some(p) = line.strip_prefix("DataPath=") {
                data_path = Some(p.trim_start_matches("./").to_string());
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&format!("Version={version}\n"));
        std::fs::write(&conf_path, out).unwrap();
        if let Some((from, to)) = edit {
            assert!(
                conf.contains("ModDrv=RawText"),
                "only RawText text can be edited"
            );
            let data = dir.join(data_path.expect("DataPath"));
            for t in ["ot", "nt"] {
                if data.join(t).is_file() {
                    edit_raw_text(&data.join(t), &data.join(format!("{t}.vss")), from, to);
                }
            }
        }
    }

    /// Add `line` (`Key=value`) to the conf of a module in repository folder `repo`.
    pub fn add_conf_line(&self, repo: &str, conf_file: &str, line: &str) {
        let conf_path = self.root.join(repo).join("mods.d").join(conf_file);
        let mut conf = std::fs::read_to_string(&conf_path).unwrap();
        conf.push_str(line);
        conf.push('\n');
        std::fs::write(&conf_path, conf).unwrap();
    }
}

/// Replace `from` with `to` in every entry of a RawText testament (6-byte index entries:
/// offset u32, length u16, little-endian), writing the text and its index again. Slots that
/// shared one stored entry (linked verses) still share it.
fn edit_raw_text(text: &Path, index: &Path, from: &str, to: &str) {
    let old = std::fs::read(text).unwrap();
    let idx = std::fs::read(index).unwrap();
    let mut data = Vec::new();
    let mut new_idx = Vec::with_capacity(idx.len());
    let mut moved: std::collections::HashMap<[u8; 6], [u8; 6]> = std::collections::HashMap::new();
    for e in idx.as_chunks::<6>().0 {
        let entry = moved.entry(*e).or_insert_with(|| {
            let off = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize;
            let len = u16::from_le_bytes([e[4], e[5]]) as usize;
            let entry = if len == 0 {
                String::new()
            } else {
                String::from_utf8(old[off..off + len].to_vec())
                    .unwrap()
                    .replace(from, to)
            };
            let mut out = [0u8; 6];
            out[..4].copy_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
            out[4..].copy_from_slice(&u16::try_from(entry.len()).unwrap().to_le_bytes());
            data.extend_from_slice(entry.as_bytes());
            out
        });
        new_idx.extend_from_slice(entry);
    }
    std::fs::write(text, data).unwrap();
    std::fs::write(index, new_idx).unwrap();
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

    /// The Psalms of the STEPBible TAHOT test excerpt (Job–Song), imported the way the
    /// Starter library imports TAHOT: Hebrew verse numbers and a Verse map, so its Ps 3:1 is
    /// the superscription (Standard Ps 3:title) and its Ps 3:2 is Standard Ps 3:1.
    pub fn tahot_psalms() -> Resource {
        let asset = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../formats/tests/assets/stepbible/tahot-job-sng.txt");
        let mut import = obs_formats::StepbibleImport::new(obs_formats::StepbibleOptions {
            text: obs_formats::StepbibleText::Tahot,
            resource_id: "tahot".into(),
            title: "TAHOT (test)".into(),
            abbreviation: "TAHOT".into(),
            source_file: "tahot-job-sng.txt".into(),
            tool_version: "test".into(),
            ..Default::default()
        });
        import.add_file("tahot-job-sng.txt", std::fs::read_to_string(asset).unwrap());
        import.finish().unwrap().0
    }

    /// Psalm 3:1-3 of the BSB (public domain) as its USFM download marks it up, with the
    /// superscription inside verse 1 (`\d \v 1`), imported as the Starter library imports
    /// the BSB: English numbering, which is the Standard versification's.
    pub fn bsb_psalm_3() -> Resource {
        let usfm = "\\id PSA\n\\c 3\n\\s1 Deliver Me, O LORD!\n\\d \\v 1 A Psalm of David, when he fled from his son Absalom.\n\\b\n\\q1 O LORD, how my foes have increased!\n\\q2 How many rise up against me!\n\\q1 \\v 2 Many say of me,\n\\q2 “God will not deliver him.”\n\\qr Selah\n\\q1 \\v 3 But You, O LORD, are a shield around me,\n\\q2 my glory, and the One who lifts my head.\n";
        let mut import = obs_formats::UsfmImport::new(obs_formats::UsfmOptions {
            resource_id: "bsb".into(),
            title: "Berean Standard Bible (test)".into(),
            abbreviation: "BSB".into(),
            language: "en".into(),
            source_file: "PSA.usfm".into(),
            tool_version: "test".into(),
            ..Default::default()
        });
        import.add_book("PSA.usfm", usfm.into()).unwrap();
        import.finish().unwrap().0
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

fn person(
    id: &str,
    kind: obs_model::EntityKind,
    relations: &[(obs_model::Relation, &str)],
) -> obs_model::Entity {
    obs_model::Entity {
        id: id.into(),
        name: id.split('@').next().unwrap().into(),
        kind,
        strongs: None,
        other_strongs: Vec::new(),
        other_names: Vec::new(),
        region: None,
        relations: relations
            .iter()
            .map(|(relation, entity)| obs_model::EntityRelation {
                relation: *relation,
                entity: (*entity).into(),
                uncertain: false,
            })
            .collect(),
    }
}

pub const BAPTIST: &str = "John@Mat.3.1-Act";
pub const APOSTLE: &str = "John@Mat.4.21-Rev";
pub const PETERS_FATHER: &str = "John@Mat.16.17-Jhn";
pub const PRIEST: &str = "John@Act.4.6";
pub const MARK: &str = "Mark@Act.12.12-1Pe";

/// The New Testament Johns as TIPNR keeps them (structured fields only): four men called
/// John, and Mark, also called John; their relatives; one mention per Verse.
pub fn johns() -> obs_model::Resource {
    use EntityKind::{Female, Male};
    use Relation::{Father, Mother, Offspring, Sibling};
    use obs_model::{EntityKind, KeyScheme, Manifest, Relation, ResourceBuilder, ResourceType};
    let mut m = Manifest::new(
        "johns",
        "People & places (Johns)",
        "PPJ",
        ResourceType::PeopleAndPlaces,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "CC-BY-4.0".into();
    let mut b = ResourceBuilder::new(m);
    let mut entities = vec![
        person(
            BAPTIST,
            Male,
            &[
                (Father, "Zechariah@Luk.1.5-"),
                (Mother, "Elizabeth@Luk.1.5-"),
            ],
        ),
        person(
            APOSTLE,
            Male,
            &[
                (Father, "Zebedee@Mat.4.21-Jhn"),
                (Sibling, "James@Mat.4.21-Act"),
            ],
        ),
        person(PETERS_FATHER, Male, &[(Offspring, "Peter@Mat.4.18-2Pe")]),
        person(PRIEST, Male, &[]),
        person(MARK, Male, &[(Mother, "Mary@Act.12.12")]),
        person("Zechariah@Luk.1.5-", Male, &[(Offspring, BAPTIST)]),
        person("Elizabeth@Luk.1.5-", Female, &[(Offspring, BAPTIST)]),
        person("Zebedee@Mat.4.21-Jhn", Male, &[(Offspring, APOSTLE)]),
        person(
            "James@Mat.4.21-Act",
            Male,
            &[(Father, "Zebedee@Mat.4.21-Jhn")],
        ),
        person("Peter@Mat.4.18-2Pe", Male, &[(Father, PETERS_FATHER)]),
        person("Mary@Act.12.12", Female, &[(Offspring, MARK)]),
        person(
            "Jerusalem@Gen.14.18-Rev",
            EntityKind::Place,
            &[(Relation::Inhabitant, PRIEST)],
        ),
    ];
    entities[3].region = Some("Tribe of Levi".into());
    entities[4].other_names = vec!["John".into(), "John Mark".into()];
    entities[4].strongs = Some("G3138".parse().unwrap());
    entities[1].strongs = Some("G2491H".parse().unwrap());
    entities[0].strongs = Some("G2491G".parse().unwrap());
    entities[2].strongs = Some("G2491K".parse().unwrap());
    entities[3].strongs = Some("G2491I".parse().unwrap());
    for e in entities {
        b.add_entity(e);
    }
    let v = |book, c, n| obs_scripture::Passage::verse(obs_scripture::Verse::new(book, c, n));
    use obs_scripture::Book;
    let mentions: &[(&str, obs_scripture::Passage)] = &[
        (BAPTIST, v(Book::Matthew, 3, 1)),
        (BAPTIST, v(Book::Matthew, 3, 4)),
        (BAPTIST, v(Book::Luke, 1, 13)),
        (BAPTIST, v(Book::Luke, 1, 60)),
        ("Zechariah@Luk.1.5-", v(Book::Luke, 1, 13)),
        ("Zechariah@Luk.1.5-", v(Book::Luke, 1, 60)),
        ("Elizabeth@Luk.1.5-", v(Book::Luke, 1, 60)),
        (APOSTLE, v(Book::Matthew, 4, 21)),
        (APOSTLE, v(Book::Mark, 1, 19)),
        (APOSTLE, v(Book::Revelation, 1, 9)),
        ("Zebedee@Mat.4.21-Jhn", v(Book::Matthew, 4, 21)),
        ("James@Mat.4.21-Act", v(Book::Matthew, 4, 21)),
        ("James@Mat.4.21-Act", v(Book::Mark, 1, 19)),
        (PETERS_FATHER, v(Book::Matthew, 16, 17)),
        (PETERS_FATHER, v(Book::John, 1, 42)),
        ("Peter@Mat.4.18-2Pe", v(Book::Matthew, 16, 17)),
        ("Peter@Mat.4.18-2Pe", v(Book::John, 1, 42)),
        (PRIEST, v(Book::Acts, 4, 6)),
        ("Jerusalem@Gen.14.18-Rev", v(Book::Acts, 4, 5)),
        (MARK, v(Book::Acts, 12, 12)),
        (MARK, v(Book::Acts, 12, 25)),
        ("Mary@Act.12.12", v(Book::Acts, 12, 12)),
    ];
    for (id, p) in mentions {
        b.add_mention(id, *p);
    }
    b.build().unwrap()
}

/// A names Dictionary laid out like Hitchcock's (upper-case headwords; public-domain text).
pub fn hitchcock() -> obs_model::Resource {
    use obs_model::{Key, KeyScheme, Manifest, ResourceBuilder, ResourceType, UnitId, UnitKind};
    let mut m = Manifest::new(
        "hitchcock",
        "Hitchcock's Bible Names",
        "Hitchcock",
        ResourceType::Dictionary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Headword];
    m.license = "LicenseRef-PublicDomain".into();
    let mut b = ResourceBuilder::new(m);
    for (head, text) in [
        ("ABRAHAM", "ABRAHAM, father of a great multitude"),
        ("JOHN", "JOHN, the grace or mercy of the Lord"),
    ] {
        let unit = UnitId::new(head.to_lowercase()).unwrap();
        b.add_unit(
            unit.clone(),
            UnitKind::Entry,
            Some(head.into()),
            text.into(),
        );
        b.add_key(Key::Headword(head.into()), &unit, None, false);
    }
    b.build().unwrap()
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

/// John 15:13 and 1 John 4:8 as STEPBible's TAGNT tags them (CC BY): G26 with its
/// Morphology, genitive once and nominative once.
pub fn tagnt_love() -> obs_model::Resource {
    use obs_scripture::{Book, Verse};
    let mut r = tagged_bible(
        "tagnt-love",
        "Translators Amalgamated Greek NT (test)",
        "TAGNT",
        "grc",
        &[
            (
                Verse::new(Book::John, 15, 13),
                "μείζονα{G3187|μείζων|A-ASF-C|Greater} ταύτης{G3778|οὗτος|D-GSF|than_this} \
                 ἀγάπης{G26|ἀγάπη|N-GSF|love} οὐδεὶς{G3762|οὐδείς|A-NSM-N|no_one} \
                 ἔχει{G2192|ἔχω|V-PAI-3S|has}",
            ),
            (
                Verse::new(Book::FirstJohn, 4, 8),
                "ὁ{G3588|ὁ|T-NSM|-} θεὸς{G2316|θεός|N-NSM-T|God} \
                 ἀγάπη{G26|ἀγάπη|N-NSF|love} ἐστίν{G1510|εἰμί|V-PAI-3S|is}.",
            ),
        ],
    );
    r.manifest.license = "CC-BY-4.0".into();
    r
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

/// A User database whose preference writes, or its next few History Visit writes, fail
/// ("disk full"); everything else is the real `store`. For checking that a failed save is
/// reported, not hidden, and that History keeps what it could not write.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct WritesFail {
    pub store: obs_store::UserStore,
    /// Every preference write fails.
    pub preferences: bool,
    /// How many Visit writes still fail before they work again.
    pub visits: std::cell::Cell<u32>,
}

#[cfg(not(target_arch = "wasm32"))]
impl WritesFail {
    /// Every preference write fails.
    pub fn preferences(store: obs_store::UserStore) -> Self {
        Self {
            store,
            preferences: true,
            visits: std::cell::Cell::new(0),
        }
    }

    /// The next `failures` Visit writes fail.
    pub fn visits(store: obs_store::UserStore, failures: u32) -> Self {
        Self {
            store,
            preferences: false,
            visits: std::cell::Cell::new(failures),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl crate::UserData for WritesFail {
    fn preference(&self, key: &str) -> Result<Option<String>, String> {
        crate::UserData::preference(&self.store, key)
    }
    fn set_preference(&self, key: &str, value: &str) -> Result<(), String> {
        if self.preferences {
            return Err("disk full".into());
        }
        crate::UserData::set_preference(&self.store, key, value)
    }
    fn last_session(&self) -> Result<Option<String>, String> {
        crate::UserData::last_session(&self.store)
    }
    fn save_last_session(&self, snapshot: &str) -> Result<(), String> {
        crate::UserData::save_last_session(&self.store, snapshot)
    }
    fn layouts(&self) -> Result<Vec<String>, String> {
        crate::UserData::layouts(&self.store)
    }
    fn layout(&self, name: &str) -> Result<Option<String>, String> {
        crate::UserData::layout(&self.store, name)
    }
    fn save_layout(&self, name: &str, snapshot: &str) -> Result<(), String> {
        crate::UserData::save_layout(&self.store, name, snapshot)
    }
    fn rename_layout(&self, from: &str, to: &str) -> Result<bool, String> {
        crate::UserData::rename_layout(&self.store, from, to)
    }
    fn delete_layout(&self, name: &str) -> Result<bool, String> {
        crate::UserData::delete_layout(&self.store, name)
    }
    fn preferred_order(&self) -> Result<Vec<String>, String> {
        crate::UserData::preferred_order(&self.store)
    }
    fn save_preferred_order(&self, ids: &[String]) -> Result<(), String> {
        crate::UserData::save_preferred_order(&self.store, ids)
    }
    fn resource_names(&self) -> Result<Vec<(String, String, String)>, String> {
        crate::UserData::resource_names(&self.store)
    }
    fn save_resource_names(&self, names: &[(String, String, String)]) -> Result<(), String> {
        crate::UserData::save_resource_names(&self.store, names)
    }
    fn favorites(&self) -> Result<Vec<obs_store::FavoriteRow>, String> {
        crate::UserData::favorites(&self.store)
    }
    fn add_favorite(&self, folder: &str, label: &str, location: &str) -> Result<i64, String> {
        crate::UserData::add_favorite(&self.store, folder, label, location)
    }
    fn remove_favorite(&self, id: i64) -> Result<bool, String> {
        crate::UserData::remove_favorite(&self.store, id)
    }
    fn move_favorite(&self, id: i64, folder: &str) -> Result<bool, String> {
        crate::UserData::move_favorite(&self.store, id, folder)
    }
    fn record_visit(&self, label: &str, location: &str) -> Result<(), String> {
        let failures = self.visits.get();
        if failures > 0 {
            self.visits.set(failures - 1);
            return Err("disk full".into());
        }
        crate::UserData::record_visit(&self.store, label, location)
    }
    fn history_as_of(&self, at: i64) -> Result<Vec<obs_store::VisitRow>, String> {
        crate::UserData::history_as_of(&self.store, at)
    }
    fn local_day(&self, at: i64) -> Result<String, String> {
        crate::UserData::local_day(&self.store, at)
    }
    fn clear_history(&self) -> Result<(), String> {
        crate::UserData::clear_history(&self.store)
    }
    fn reading_plans(&self) -> Result<Vec<obs_store::ReadingPlanRow>, String> {
        crate::UserData::reading_plans(&self.store)
    }
    fn plan_readings(&self, plan_id: i64) -> Result<Vec<obs_store::PlanReadingRow>, String> {
        crate::UserData::plan_readings(&self.store, plan_id)
    }
    fn add_reading_plan(
        &self,
        plan: &obs_store::ReadingPlanRow,
        readings: &[obs_store::PlanReadingRow],
    ) -> Result<i64, String> {
        crate::UserData::add_reading_plan(&self.store, plan, readings)
    }
    fn set_reading_read(&self, plan_id: i64, position: u32, read: bool) -> Result<bool, String> {
        crate::UserData::set_reading_read(&self.store, plan_id, position, read)
    }
    fn move_readings(&self, plan_id: i64, days: &[(u32, String)]) -> Result<(), String> {
        crate::UserData::move_readings(&self.store, plan_id, days)
    }
    fn remove_reading_plan(&self, id: i64) -> Result<bool, String> {
        crate::UserData::remove_reading_plan(&self.store, id)
    }
    fn notebooks(&self) -> Result<Vec<obs_store::Notebook>, String> {
        crate::UserData::notebooks(&self.store)
    }
    fn notebook_counts(&self) -> Result<Vec<(String, u64)>, String> {
        crate::UserData::notebook_counts(&self.store)
    }
    fn annotation(&self, id: &str) -> Result<Option<obs_store::Annotation>, String> {
        crate::UserData::annotation(&self.store, id)
    }
    fn annotations(&self, notebook: Option<&str>) -> Result<Vec<obs_store::Annotation>, String> {
        crate::UserData::annotations(&self.store, notebook)
    }
    fn annotations_referring(&self, resource_id: &str) -> Result<u64, String> {
        crate::UserData::annotations_referring(&self.store, resource_id)
    }
    fn search_notes(&self, fts_query: &str) -> Result<Vec<String>, String> {
        crate::UserData::search_notes(&self.store, fts_query)
    }
    fn apply_changes(&self, changes: &[obs_store::Change]) -> Result<(), String> {
        crate::UserData::apply_changes(&self.store, changes)
    }
    fn annotation_dump(&self) -> Result<String, String> {
        crate::UserData::annotation_dump(&self.store)
    }
}

/// A small Starter library (#131): a KJV-style Bible with the Starter id `kjv-strongs`
/// (Genesis 1:1-3, public domain) and the Starter Lexicon excerpts (TBESH and TBESG under
/// CC BY 4.0, BDB, Strong's Hebrew), each marked as built by `xtask starter`.
#[cfg(not(target_arch = "wasm32"))]
pub fn starter_set() -> Vec<obs_model::Resource> {
    use obs_scripture::{Book, Verse};
    let verses = [
        (
            Verse::new(Book::Genesis, 1, 1),
            "In the beginning God created the heaven and the earth.",
        ),
        (
            Verse::new(Book::Genesis, 1, 2),
            "And the earth was without form, and void; and darkness was upon the face of the deep.",
        ),
        (
            Verse::new(Book::Genesis, 1, 3),
            "And God said, Let there be light: and there was light.",
        ),
    ];
    let mut kjv = bibles::bible("kjv-strongs", "KJV", &verses, None, &[]);
    kjv.manifest.license = "LicenseRef-PublicDomain".into();
    let mut set = vec![kjv];
    for mut r in starter_lexicon_excerpts() {
        r.manifest.license = if r.manifest.resource_id.starts_with("tbes") {
            "CC-BY-4.0".into()
        } else {
            "LicenseRef-PublicDomain".into()
        };
        set.push(r);
    }
    for r in &mut set {
        r.manifest.origin = obs_model::Origin::Starter;
    }
    set
}

/// Write `resources` into `dir` as `cargo xtask starter` does: one `.obr` each, its prebuilt
/// Search index cache in `search/`, a change record for each CC BY one, and the
/// `starter-manifest.json` naming each file's sha256 (#131).
#[cfg(not(target_arch = "wasm32"))]
pub fn write_starter_bundle(dir: &Path, resources: &[obs_model::Resource]) {
    std::fs::create_dir_all(dir).unwrap();
    let mut items = Vec::new();
    for r in resources {
        let id = &r.manifest.resource_id;
        let path = dir.join(format!("{id}.obr"));
        obs_store::write_obr(r, &path).unwrap();
        obs_store::build_search_index(
            r,
            obs_search::INDEX_VERSION,
            &dir.join("search"),
            &mut |_, _| true,
        )
        .unwrap();
        if obs_model::is_cc_by(&r.manifest.license) {
            std::fs::write(
                dir.join(obs_model::changes_record_file(id)),
                format!("# Changes we made: {id}\n"),
            )
            .unwrap();
        }
        let bytes = std::fs::read(&path).unwrap();
        let m = &r.manifest;
        items.push(serde_json::json!({
            "resource_id": id,
            "revision": m.revision,
            "content_hash": m.content_hash,
            "file_sha256": crate::starter::sha256_hex(&bytes),
            "bytes": bytes.len(),
            "title": m.title,
            "abbreviation": m.abbreviation,
            "resource_type": m.resource_type,
            "language": m.language,
            "license": m.license,
            "units": r.units.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(),
            "unit_aliases": {},
        }));
    }
    let manifest = serde_json::json!({ "format": 1, "resources": items });
    std::fs::write(
        dir.join(crate::starter::MANIFEST_FILE),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

/// A zip of `(name, text)` files, as a publisher's `*_usfm.zip`.
pub fn zip_of(files: &[(String, String)]) -> Vec<u8> {
    use std::io::Write;
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, text) in files {
        zip.start_file(name.as_str(), options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    out.into_inner()
}

/// The licence notice the mock Open catalogue's zip carries, as eBible's `copr.htm` does.
pub const MOCK_OPEN_NOTICE: &str =
    "<html><body><h1>World English Bible</h1><p>Public Domain</p></body></html>";

/// A mock Open catalogue (#134): one entry, `web-excerpt`, an eBible-style USFM zip at a
/// `file://` URL under `root`, built from the open-text WEB test excerpt with `notice` as its
/// `copr.htm`. Its other fields are the compiled-in catalogue's first entry's.
pub fn mock_open_catalogue(root: &Path, notice: &str) -> Vec<crate::open_catalogue::OpenSource> {
    let usfm = Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets/usfm/web");
    let mut files: Vec<(String, String)> = std::fs::read_dir(&usfm)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let name = format!("{}webx.usfm", p.file_stem().unwrap().to_string_lossy());
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect();
    files.sort();
    files.push(("copr.htm".into(), notice.into()));
    std::fs::create_dir_all(root).unwrap();
    let zip = root.join("webx_usfm.zip");
    std::fs::write(&zip, zip_of(&files)).unwrap();
    let mut source = crate::open_catalogue::open_sources()[0].clone();
    source.id = "web-excerpt".into();
    source.url = crate::credits::file_url(&zip);
    source.title = "World English Bible excerpt".into();
    source.abbreviation = "WEBX".into();
    source.language = "en".into();
    source.publisher = "eBible.org".into();
    source.sha256 = None;
    source.licence_file = Some("copr.htm".into());
    source.licence_phrase = Some("Public Domain".into());
    vec![source]
}
