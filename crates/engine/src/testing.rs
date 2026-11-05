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
