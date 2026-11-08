//! The scale scenario (#227): the app over a library of [`SCALE_RESOURCES`] Resources, as Get
//! Resources makes possible. `openbible --bench --scale` ([`write_scale_library`], then
//! [`run`] in a fresh process) measures:
//!
//! - **cold start**: from the process's `main` to the first frame that draws the ★ Bible's
//!   text, through the app's own launch (`App::with_file`: library scan, Search index and
//!   alias checks, User database, Last session);
//! - **Library Tool filtering**: each key typed into the Library Tab's Filter box, from the
//!   key to the frame showing the filtered list, and the list scrolled;
//! - **Get Resources**: its catalog filtered as a search is typed, and scrolled;
//! - **Library search** across every Commentary, Lexicon and Dictionary, until its results;
//! - **Passage Guide** gathering every section for a Passage;
//! - **memory**: the process's resident set (RSS) with 6 Panels open, and its peak.
//!
//! The library is generated, deterministically, into a folder of its own: a few template
//! Resources of realistic sizes (whole Bibles and New Testaments of about 25 words a verse,
//! Commentaries, Dictionaries and Lexicons like the Starter library's) are built once with
//! their Search index caches, then copied under [`SCALE_RESOURCES`] identities
//! ([`obs_store::copy_obr_as`]). Every app folder (library, User database, caches, logs) is
//! inside it ([`ScaleDirs::env`]), so the run never touches the user's own.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use obs_engine::TabKind;
use obs_model::{
    Key, KeyScheme, Manifest, ParaStyle, Resource, ResourceBuilder, ResourceType, SpanKind, UnitId,
    UnitKind,
};
use obs_scripture::{Book, Passage, Standard, Testament, Verse, Versification};
use serde::Serialize;
use serde_json::json;

use super::library::{self, Rng};
use super::tools::{ActionWork, ShellBench};
use super::{BenchConfig, Samples, ScenarioReport};

/// Resources in the scale library: Get Resources lists about 2,200 modules.
pub const SCALE_RESOURCES: usize = 2_000;

/// Bump when the generated library changes, so one made by an older build is made again.
const GENERATOR_VERSION: u32 = 3;

/// The share of each kind of Resource in the library, in thousandths, from the live SWORD
/// repositories Get Resources lists (eBible.org's 1,530 modules are nearly all Bibles, most of
/// them New Testaments): Bibles, then Commentaries, Dictionaries and Lexicons.
const MIX: [(Template, u32); 5] = [
    (Template::WholeBible, 352),
    (Template::NewTestament, 528),
    (Template::Commentary, 60),
    (Template::Dictionary, 40),
    (Template::Lexicon, 20),
];

/// Distinct contents of each kind: copies of one template share its text.
const VARIANTS: u64 = 3;

/// What the typed Library filter and Get Resources search look for, a key at a time.
const LIBRARY_QUERY: &str = "rabe";

/// What the Library search looks for, unless the run is given another word: about as common
/// as in the Starter library ([`LORD_IN_COMMENTARY`]).
pub const LIBRARY_SEARCH: &str = "lord";

/// Panels open when memory is measured (#19: RSS ≤ 500 MB with 6 Panels open).
pub const PANELS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Template {
    WholeBible,
    NewTestament,
    Commentary,
    Dictionary,
    Lexicon,
}

impl Template {
    fn id(self) -> &'static str {
        match self {
            Template::WholeBible | Template::NewTestament => "bible",
            Template::Commentary => "commentary",
            Template::Dictionary => "dictionary",
            Template::Lexicon => "lexicon",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Template::WholeBible => "Bible",
            Template::NewTestament => "New Testament",
            Template::Commentary => "Commentary",
            Template::Dictionary => "Bible Dictionary",
            Template::Lexicon => "Lexicon",
        }
    }

    fn build(self, seed: u64) -> Option<Resource> {
        let id = format!(
            "template-{}-{seed}",
            self.noun().to_lowercase().replace(' ', "-")
        );
        match self {
            // The first whole Bible has Strong's numbers, like the KJV with Strong's: so has
            // the ★ Bible, and about one Bible in eight, as in the SWORD repositories.
            Template::WholeBible => {
                library::english_bible(&id, "TB", &Book::ALL, 10..41, seed == 1, seed)
            }
            Template::NewTestament => {
                let nt: Vec<Book> = Book::ALL
                    .into_iter()
                    .filter(|b| b.testament() == Testament::New)
                    .collect();
                library::english_bible(&id, "TNT", &nt, 10..41, false, seed)
            }
            Template::Commentary => commentary(&id, seed),
            Template::Dictionary => dictionary(&id, seed),
            Template::Lexicon => lexicon(&id, seed),
        }
    }
}

/// The folders and files of a scale run, all under `root`.
#[derive(Debug, Clone)]
pub struct ScaleDirs {
    pub root: PathBuf,
}

impl ScaleDirs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn library(&self) -> PathBuf {
        self.root.join("library")
    }

    fn templates(&self) -> PathBuf {
        self.root.join("templates")
    }

    fn search(&self) -> PathBuf {
        self.root.join("cache").join("search")
    }

    fn stamp(&self) -> PathBuf {
        self.root.join("stamp")
    }

    /// The environment the app finds its folders in: every one inside `root`, so a run reads
    /// only the scale library and never writes to the user's own. A Starter library folder
    /// without a bundle keeps the Starter library out.
    pub fn env(&self) -> Vec<(&'static str, PathBuf)> {
        let r = &self.root;
        let home = r.join("home");
        let mut starter_url = std::ffi::OsString::from("file://");
        starter_url.push(r.join("no-starter"));
        vec![
            ("OBS_LIBRARY", self.library()),
            ("OBS_USER_DB", r.join("user.db")),
            ("OBS_SEARCH_CACHE", self.search()),
            ("OBS_ALIAS_CACHE", r.join("cache").join("aliases")),
            ("OBS_SWORD_CATALOG", r.join("sword-catalog.json")),
            ("OBS_SWORD_DIR", r.join("sword")),
            ("OBS_LOG_DIR", r.join("logs")),
            ("OBS_STARTER_DIR", r.join("no-starter")),
            ("OBS_STARTER_URL", PathBuf::from(starter_url)),
            ("HOME", home.clone()),
            ("XDG_DATA_HOME", home.join("data")),
            ("XDG_CACHE_HOME", home.join("cache")),
            ("APPDATA", home.join("data")),
            ("LOCALAPPDATA", home.join("cache")),
        ]
    }
}

/// The library [`write_scale_library`] left.
#[derive(Debug, Clone, Serialize)]
pub struct ScaleLibrary {
    /// Resources in the library: the generated ones and a People & places index.
    pub resources: usize,
    /// `.obr` files and Search index caches, bytes.
    pub bytes: u64,
    /// Whether it was made now (else one made earlier for the same build was kept).
    pub made: bool,
    pub seconds: f64,
}

fn stamp_text(resources: usize) -> String {
    format!(
        "scale library v{GENERATOR_VERSION}, {resources} Resources, Search index v{}",
        obs_search::INDEX_VERSION
    )
}

fn folder_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .filter(std::fs::Metadata::is_file)
        .map(|m| m.len())
        .sum()
}

/// The kind of the `n`th Resource: [`MIX`] spread evenly through the list (every 1,000 in a
/// row hold it exactly), the first (the ★ Bible) a whole Bible.
fn kind_of(n: usize) -> Template {
    // `n`'s slot in a cycle of 1,000 that 7919 (a prime) visits in a scattered order.
    let slot = u32::try_from((n * 7919) % 1000).unwrap_or(0);
    let mut acc = 0;
    for (kind, share) in MIX {
        acc += share;
        if slot < acc {
            return kind;
        }
    }
    Template::WholeBible
}

/// A made-up word of `min` to `min + spread - 1` syllables.
fn word(rng: &mut Rng, min: u64, spread: u64) -> String {
    let syllables = min + rng.below(spread);
    library::made_up_word(rng, syllables)
}

/// Languages of the library: English most, then the 60 of Get Resources' catalog.
fn language_of(rng: &mut Rng) -> String {
    match rng.below(10) {
        0..=3 => "en".into(),
        4 => "grc".into(),
        5 => "hbo".into(),
        _ => format!("x{}", library::made_up_word(rng, 1)),
    }
}

/// A template Resource written to `obr`, with its Search index cache at `index`.
struct BuiltTemplate {
    kind: Template,
    seed: u64,
    obr: PathBuf,
    index: PathBuf,
    manifest: Manifest,
}

/// Write the scale library of `resources` Resources under `dirs` (see the module docs), with
/// a Get Resources catalog and a prepared User database. A library already there from the
/// same build, of the same size, is kept.
pub fn write_scale_library(dirs: &ScaleDirs, resources: usize) -> Result<ScaleLibrary, String> {
    let start = Instant::now();
    let stamp = stamp_text(resources);
    let bytes = || folder_bytes(&dirs.library()) + folder_bytes(&dirs.search());
    if std::fs::read_to_string(dirs.stamp()).is_ok_and(|s| s == stamp) {
        return Ok(ScaleLibrary {
            resources: resources + 1,
            bytes: bytes(),
            made: false,
            seconds: start.elapsed().as_secs_f64(),
        });
    }
    let fail = |what: &Path, e: &dyn std::fmt::Display| format!("{}: {e}", what.display());
    if dirs.root.exists() {
        std::fs::remove_dir_all(&dirs.root).map_err(|e| fail(&dirs.root, &e))?;
    }
    for d in [dirs.library(), dirs.templates(), dirs.search()] {
        std::fs::create_dir_all(&d).map_err(|e| fail(&d, &e))?;
    }
    // The templates, each with its Search index cache, built on a few threads.
    let kinds: Vec<(Template, u64)> = MIX
        .iter()
        .flat_map(|(k, _)| (0..VARIANTS).map(move |v| (*k, v + 1)))
        .collect();
    let templates: Vec<Result<BuiltTemplate, String>> = std::thread::scope(|s| {
        let jobs: Vec<_> = kinds
            .chunks(kinds.len().div_ceil(4))
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|&(kind, seed)| {
                            let r = kind.build(seed).ok_or("a template is invalid")?;
                            let obr = dirs
                                .templates()
                                .join(format!("{}.obr", r.manifest.resource_id));
                            obs_store::write_obr(&r, &obr).map_err(|e| fail(&obr, &e))?;
                            let index = obs_store::build_search_index(
                                &r,
                                obs_search::INDEX_VERSION,
                                &dirs.templates().join("search"),
                                &mut |_, _| true,
                            )
                            .map_err(|e| fail(&obr, &e))?;
                            Ok(BuiltTemplate {
                                kind,
                                seed,
                                obr,
                                index,
                                manifest: r.manifest,
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        jobs.into_iter()
            .flat_map(|j| j.join().unwrap_or_default())
            .collect()
    });
    let templates = templates.into_iter().collect::<Result<Vec<_>, _>>()?;
    // The copies: each its own Resource id, title, abbreviation and language.
    let mut rng = Rng(0x5ca1e);
    let plan: Vec<(PathBuf, PathBuf, Manifest)> = (0..resources)
        .map(|n| {
            let kind = kind_of(n);
            let variant = 1 + (n as u64 / 7) % VARIANTS;
            let t = templates
                .iter()
                .find(|t| t.kind == kind && t.seed == variant)
                .ok_or_else(|| format!("no {kind:?} template"))?;
            let mut m = t.manifest.clone();
            let name = library::capitalized(&word(&mut rng, 2, 2));
            m.resource_id = format!("{}-{n:04}", kind.id());
            m.language = language_of(&mut rng);
            m.title = format!("{name} {} ({})", kind.noun(), m.language);
            m.abbreviation = format!("{}{n}", name.to_uppercase());
            Ok((t.obr.clone(), t.index.clone(), m))
        })
        .collect::<Result<_, String>>()?;
    let copied: Vec<Result<(), String>> = std::thread::scope(|s| {
        let jobs: Vec<_> = plan
            .chunks(plan.len().div_ceil(4).max(1))
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|(obr, index, m)| {
                            let to = dirs.library().join(format!("{}.obr", m.resource_id));
                            obs_store::copy_obr_as(obr, &to, m).map_err(|e| fail(&to, &e))?;
                            obs_store::copy_search_index_as(index, &dirs.search(), m)
                                .map_err(|e| fail(index, &e))?;
                            Ok(())
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        jobs.into_iter()
            .flat_map(|j| j.join().unwrap_or_default())
            .collect()
    });
    copied.into_iter().collect::<Result<Vec<()>, String>>()?;
    // A People & places index, as the Starter library has (the Passage Guide's People & places
    // section, and alias tables with every Dictionary).
    library::write_library(
        &dirs.library(),
        &[library::places().ok_or("the places index is invalid")?],
    )?;
    // Get Resources' catalog, as last fetched.
    let fetched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let catalog = dirs.root.join("sword-catalog.json");
    let json = serde_json::to_string(&library::catalog(fetched_at)).map_err(|e| e.to_string())?;
    std::fs::write(&catalog, json).map_err(|e| fail(&catalog, &e))?;
    std::fs::remove_dir_all(dirs.templates()).map_err(|e| fail(&dirs.templates(), &e))?;
    std::fs::write(dirs.stamp(), &stamp).map_err(|e| fail(&dirs.stamp(), &e))?;
    Ok(ScaleLibrary {
        resources: resources + 1,
        bytes: bytes(),
        made: true,
        seconds: start.elapsed().as_secs_f64(),
    })
}

/// Paragraphs in a thousand that name the LORD, as in the Starter library: Matthew Henry's
/// Commentary 277, Jamieson-Fausset-Brown 69; Easton's Bible Dictionary 71, Smith's 39; the
/// Lexicons fewer than 3. The Library search ([`LIBRARY_SEARCH`]) finds as many.
const LORD_IN_COMMENTARY: u64 = 150;
const LORD_IN_DICTIONARY: u64 = 55;
const LORD_IN_LEXICON: u64 = 3;

/// `words` words of a Commentary, Dictionary or Lexicon paragraph: six in ten common words
/// (never LORD), the rest made up, then LORD in `lord` paragraphs of a thousand.
fn paragraph(rng: &mut Rng, words: u64, lord: u64) -> String {
    let mut out: Vec<String> = (0..words)
        .map(|_| match rng.below(10) {
            0..=5 => loop {
                let w = *rng.pick(library::WORDS);
                if w != "LORD" {
                    break w.to_string();
                }
            },
            _ => word(rng, 1, 3),
        })
        .collect();
    if rng.below(1000) < lord {
        let at = usize::try_from(rng.below(out.len() as u64)).unwrap_or(0);
        out.insert(at, "LORD".into());
    }
    out.join(" ")
}

/// A Commentary on the whole Bible (about JFB's size): a unit per Bible book, a section every
/// 1–6 Verses of 60–200 words, each with its Passage Key.
fn commentary(id: &str, seed: u64) -> Option<Resource> {
    let mut m = Manifest::new(
        id,
        "Template Commentary",
        "TC",
        ResourceType::Commentary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let mut rng = Rng(seed ^ 0xc033);
    for book in Book::ALL {
        let unit = UnitId::book(book);
        let mut text = String::new();
        let mut sections = Vec::new();
        for chapter in 1..=Standard.chapter_count(book) {
            let last = Standard.last_verse(book, chapter).unwrap_or(1);
            let mut verse = 1;
            while verse <= last {
                let end = (verse + u16::try_from(rng.below(6)).unwrap_or(0)).min(last);
                let start = text.len();
                text.push_str(&format!("{chapter}:{verse}. "));
                let words = 60 + rng.below(141);
                text.push_str(&paragraph(&mut rng, words, LORD_IN_COMMENTARY));
                text.push_str(".\n");
                if let Ok(p) = Passage::new(
                    Verse::new(book, chapter, verse),
                    Verse::new(book, chapter, end),
                ) {
                    sections.push((start..text.len() - 1, p));
                }
                verse = end + 1;
            }
        }
        b.add_unit(
            unit.clone(),
            UnitKind::BibleBook,
            Some(book.name().into()),
            text,
        );
        for (r, p) in sections {
            b.add_span(
                &unit,
                r.clone(),
                SpanKind::Para {
                    style: ParaStyle::Prose,
                },
            );
            b.add_key(Key::Passage(p), &unit, Some(r), false);
        }
    }
    b.build().ok()
}

/// A Bible Dictionary (about Easton's size): 4,000 articles of 30–300 words.
fn dictionary(id: &str, seed: u64) -> Option<Resource> {
    let mut m = Manifest::new(
        id,
        "Template Dictionary",
        "TD",
        ResourceType::Dictionary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Headword];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let mut rng = Rng(seed ^ 0xd1c7);
    for n in 0..4_000 {
        let head = format!("{}{n}", library::capitalized(&word(&mut rng, 2, 2)));
        let Ok(unit) = UnitId::new(format!("e{n}")) else {
            continue;
        };
        let words = 30 + rng.below(271);
        let text = format!(
            "{head}\n{}.",
            paragraph(&mut rng, words, LORD_IN_DICTIONARY)
        );
        let body_start = head.len() + 1;
        let end = text.len();
        b.add_unit(unit.clone(), UnitKind::Entry, Some(head.clone()), text);
        b.add_span(&unit, 0..head.len(), SpanKind::Heading { level: 2 });
        b.add_span(
            &unit,
            body_start..end,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
        b.add_key(Key::Headword(head), &unit, None, false);
    }
    b.build().ok()
}

/// A Greek Lexicon (about Strong's size): an entry of 20–120 words for each of 5,600 Strong's
/// numbers.
fn lexicon(id: &str, seed: u64) -> Option<Resource> {
    let mut m = Manifest::new(id, "Template Lexicon", "TL", ResourceType::Lexicon, "en");
    m.key_schemes = vec![KeyScheme::StrongsNumber];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let mut rng = Rng(seed ^ 0x1e81);
    for n in 1..=5_600u32 {
        let (Ok(strongs), Ok(unit)) = (format!("G{n}").parse(), UnitId::new(format!("G{n}")))
        else {
            continue;
        };
        let head = word(&mut rng, 2, 2);
        let words = 20 + rng.below(101);
        let text = format!("{head}\n{}.", paragraph(&mut rng, words, LORD_IN_LEXICON));
        let end = text.len();
        b.add_unit(unit.clone(), UnitKind::Entry, Some(head.clone()), text);
        b.add_span(
            &unit,
            head.len() + 1..end,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
        b.add_key(Key::StrongsNumber(strongs), &unit, None, false);
    }
    b.build().ok()
}

/// The app launched over the library as a returning user's was last left, before each run:
/// the Starter library offered (and declined) on the first launch, the default layout saved
/// as the Last session (the run before opened more). Run in a process with
/// [`ScaleDirs::env`].
pub fn prepare_launch() -> Result<(), String> {
    let mut app = crate::App::with_file(None);
    let shell = app.shell();
    shell.reset_layout();
    shell.save_session(true);
    Ok(())
}

/// What [`run`] measured.
#[derive(Debug, Clone, Serialize)]
pub struct ScaleReport {
    pub refresh_hz: f32,
    /// Resources in the library.
    pub resources: usize,
    /// From `main` to the first frame drawing the ★ Bible's text (#19: ≤ 1.5 s on the floor
    /// machine with the Starter library; the scale gate allows twice that).
    pub cold_start_ms: f64,
    /// Of which the app's launch before its first frame (`App::with_file`).
    pub launch_ms: f64,
    /// Frames from the first until the Bible's text showed.
    pub first_text_frames: usize,
    /// Each key typed into the Library Tab's Filter box, from the key to the frame showing
    /// the filtered list: the slowest, and the median.
    pub library_filter_ms_max: f64,
    pub library_filter_ms_p50: f64,
    /// The Library Tab typed into, then scrolled.
    pub library: Option<ScenarioReport>,
    /// Get Resources' catalog filtered a key at a time, from the key to its frame.
    pub get_resources_filter_ms_max: f64,
    pub get_resources: Option<ScenarioReport>,
    /// A Library search for `library_search` in every Commentary, Lexicon and Dictionary:
    /// how long until its results, how many Resources it searched and how many paragraphs it
    /// found.
    pub library_search: String,
    pub library_search_ms: f64,
    pub library_search_resources: usize,
    pub library_search_hits: usize,
    /// A Passage Guide opened, until every section is gathered; then moved to another
    /// Passage, until gathered again (the Commentaries' sections are read once).
    pub passage_guide_ms: f64,
    pub passage_guide_moved_ms: f64,
    /// Panels open when memory was measured.
    pub panels: usize,
    /// Resident set when the run ended, and its peak over the run, MiB.
    pub rss_mib: f64,
    pub peak_rss_mib: f64,
    /// The resident set after each step, MiB: where memory goes.
    pub rss_after: Vec<(String, f64)>,
    /// Steps that could not be set up or run (the gate fails on any).
    pub errors: Vec<String>,
}

/// The process's resident set now and at its peak, MiB. Linux tells the peak; elsewhere the
/// peak is the current resident set, and [`PeakSampler`] samples it through the run.
pub fn rss_mib() -> (f64, f64) {
    let kib = |field: &str, status: &str| {
        status
            .lines()
            .find_map(|l| l.strip_prefix(field))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok())
    };
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let now = kib("VmRSS:", &status).unwrap_or(0.0) / 1024.0;
        let peak = kib("VmHWM:", &status).map_or(now, |p| p / 1024.0);
        return (now, peak);
    }
    // macOS and others: `ps` tells the current resident set only.
    let now = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map_or(0.0, |k| k / 1024.0);
    (now, now)
}

/// Samples the resident set every [`PeakSampler::EVERY`] on a thread of its own until
/// dropped, keeping its peak: between the steps of a run, where the platform does not tell it
/// (macOS, whose `ps` tells only the current resident set).
struct PeakSampler {
    peak: std::sync::Arc<std::sync::Mutex<f64>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PeakSampler {
    const EVERY: Duration = Duration::from_millis(250);

    fn start() -> Self {
        let peak = std::sync::Arc::new(std::sync::Mutex::new(0.0f64));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread = {
            let (peak, stop) = (std::sync::Arc::clone(&peak), std::sync::Arc::clone(&stop));
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let (now, p) = rss_mib();
                    let mut max = peak.lock().unwrap_or_else(|e| e.into_inner());
                    *max = max.max(now).max(p);
                    drop(max);
                    std::thread::sleep(Self::EVERY);
                }
            })
        };
        Self {
            peak,
            stop,
            thread: Some(thread),
        }
    }

    fn peak(&self) -> f64 {
        *self.peak.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for PeakSampler {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Run the scale scenario in a process with [`ScaleDirs::env`] over a library from
/// [`write_scale_library`]; `started` is when the process's `main` began. The Library search
/// looks for `search` ([`LIBRARY_SEARCH`] normally).
pub fn run(started: Instant, cfg: &BenchConfig, search: &str) -> ScaleReport {
    let app = crate::App::with_file(None);
    let launch_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut report = ScaleReport {
        refresh_hz: cfg.refresh_hz,
        resources: app.shell.session().library.entries().len(),
        cold_start_ms: 0.0,
        launch_ms,
        first_text_frames: 0,
        library_filter_ms_max: 0.0,
        library_filter_ms_p50: 0.0,
        library: None,
        get_resources_filter_ms_max: 0.0,
        get_resources: None,
        library_search: search.to_string(),
        library_search_ms: 0.0,
        library_search_resources: 0,
        library_search_hits: 0,
        passage_guide_ms: 0.0,
        passage_guide_moved_ms: 0.0,
        panels: 0,
        rss_mib: 0.0,
        peak_rss_mib: 0.0,
        rss_after: Vec::new(),
        errors: Vec::new(),
    };
    let sampler = PeakSampler::start();
    let mut b = ShellBench::over(cfg, app.shell);
    let mut peak = 0.0f64;
    let sample = |report: &mut ScaleReport, peak: &mut f64, step: &str| {
        let (now, p) = rss_mib();
        *peak = peak.max(p).max(now);
        report.rss_after.push((step.into(), now));
        now
    };
    sample(&mut report, &mut peak, "launch");
    if let Err(e) = cold_start(&mut b, started, &mut report) {
        report.errors.push(format!("cold start: {e}"));
    }
    sample(&mut report, &mut peak, "cold start");
    match library_filter(&mut b, cfg) {
        Ok((typed, s)) => {
            report.library_filter_ms_max = typed.iter().copied().fold(0.0, f64::max);
            report.library_filter_ms_p50 = super::percentile(&typed, 0.5);
            report.library = Some(s.report("library", 1000.0 / f64::from(cfg.refresh_hz)));
        }
        Err(e) => report.errors.push(format!("library: {e}")),
    }
    sample(&mut report, &mut peak, "library");
    match get_resources(&mut b, cfg) {
        Ok((typed, s)) => {
            report.get_resources_filter_ms_max = typed.iter().copied().fold(0.0, f64::max);
            report.get_resources =
                Some(s.report("get-resources", 1000.0 / f64::from(cfg.refresh_hz)));
        }
        Err(e) => report.errors.push(format!("get-resources: {e}")),
    }
    sample(&mut report, &mut peak, "get resources");
    match library_search(&mut b, search) {
        Ok((ms, resources, hits)) => {
            report.library_search_ms = ms;
            report.library_search_resources = resources;
            report.library_search_hits = hits;
        }
        Err(e) => report.errors.push(format!("library search: {e}")),
    }
    sample(&mut report, &mut peak, "library search");
    match passage_guide(&mut b) {
        Ok((opened, moved)) => {
            report.passage_guide_ms = opened;
            report.passage_guide_moved_ms = moved;
        }
        Err(e) => report.errors.push(format!("passage guide: {e}")),
    }
    sample(&mut report, &mut peak, "passage guide");
    match open_panels(&mut b) {
        Ok(n) => report.panels = n,
        Err(e) => report.errors.push(format!("panels: {e}")),
    }
    report.rss_mib = sample(&mut report, &mut peak, "panels");
    report.peak_rss_mib = peak.max(sampler.peak());
    report
}

/// The Panels of the main window.
fn panels(b: &ShellBench) -> usize {
    b.shell
        .tree
        .tiles
        .tiles()
        .filter(|t| {
            matches!(
                t,
                egui_tiles::Tile::Container(egui_tiles::Container::Tabs(_))
            )
        })
        .count()
}

/// Frames until the ★ Bible draws its text: cold start ends then.
fn cold_start(
    b: &mut ShellBench,
    started: Instant,
    report: &mut ScaleReport,
) -> Result<(), String> {
    let bible = b
        .shell
        .session()
        .workspace
        .tabs()
        .find(|t| matches!(t.kind, TabKind::Resource(_)))
        .map(|t| t.id)
        .ok_or("the Last session has no Bible Tab")?;
    for frame in 0..super::SETTLE_LIMIT {
        b.frame(None, |_| {});
        if b.shell.reading_text_shown(bible) {
            report.cold_start_ms = started.elapsed().as_secs_f64() * 1000.0;
            report.first_text_frames = frame;
            return Ok(());
        }
    }
    Err(format!(
        "the Bible showed no text in {} frames",
        super::SETTLE_LIMIT
    ))
}

/// The time of each frame from `from` on, summed: how long an action took to show.
fn since(s: &Samples, from: usize) -> f64 {
    s.cpu[from.min(s.cpu.len())..].iter().sum()
}

/// Type `query` a key at a time into the field showing `hint` inside `rect`, then erase it:
/// each key's time from its frame to the frame showing it.
fn type_and_erase(
    b: &mut ShellBench,
    s: &mut Samples,
    rect: egui::Rect,
    hint: &str,
    query: &str,
) -> Result<Vec<f64>, String> {
    for _ in 0..10 {
        b.frame(None, |_| {});
    }
    let at = b
        .texts
        .iter()
        .find(|(r, t)| t == hint && rect.contains(r.center()))
        .map(|(r, _)| r.center())
        .ok_or_else(|| format!("no {hint:?} box"))?;
    b.click(at);
    let mut typed = Vec::new();
    let mut key = |b: &mut ShellBench, s: &mut Samples, event: egui::Event, shows: &str| {
        b.events.push(event);
        let from = s.cpu.len();
        let shows = shows.to_string();
        let frames = b.settle(s, ActionWork::Budgeted, |_| {}, |b| b.has_text(&shows));
        s.settle.push(frames);
        typed.push(since(s, from));
    };
    for n in 1..=query.len() {
        key(b, s, egui::Event::Text(query[n - 1..n].into()), &query[..n]);
    }
    for n in (0..query.len()).rev() {
        let backspace = egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let shows = if n == 0 { hint } else { &query[..n] };
        key(b, s, backspace, shows);
    }
    Ok(typed)
}

/// The Library Tab of the default layout: its Filter typed into and erased, then its list
/// scrolled.
fn library_filter(b: &mut ShellBench, cfg: &BenchConfig) -> Result<(Vec<f64>, Samples), String> {
    let tab = b.tab_of(&TabKind::Library).ok_or("no Library Tab")?;
    b.shell.session_mut().workspace.focus(tab);
    b.shell.apply_effects();
    b.time_tab(tab);
    let pane = b.pane(tab).ok_or("the Library is not drawn")?;
    let mut s = Samples::default();
    let typed = type_and_erase(b, &mut s, pane, "Filter", LIBRARY_QUERY)?;
    let over = egui::pos2(pane.center().x, pane.top() + pane.height() * 0.6);
    b.wheel(&mut s, over, -60.0, cfg.tool_frames);
    b.wheel(&mut s, over, 60.0, cfg.tool_frames / 2);
    Ok((typed, b.measured(s)?))
}

/// Get Resources beside the Library: its search typed and erased, then the catalog scrolled.
fn get_resources(b: &mut ShellBench, cfg: &BenchConfig) -> Result<(Vec<f64>, Samples), String> {
    let tab = b.open(
        "library.get_resources",
        &json!({ "type": "all", "split": true }),
    )?;
    b.time_tab(tab);
    let hint = "Search by name or abbreviation";
    b.wait("the catalog", |b| {
        b.shell.session().get_bibles.catalog().is_some() && b.has_text(hint)
    })?;
    let pane = b.pane(tab).ok_or("Get Resources is not drawn")?;
    let mut s = Samples::default();
    let typed = type_and_erase(b, &mut s, pane, hint, LIBRARY_QUERY)?;
    let over = egui::pos2(pane.center().x, pane.top() + pane.height() * 0.7);
    b.wheel(&mut s, over, -50.0, cfg.tool_frames);
    Ok((typed, b.measured(s)?))
}

/// A Library search for `text` in every Commentary, Lexicon and Dictionary: milliseconds
/// until its results, how many Resources it searched and how many paragraphs it found.
fn library_search(b: &mut ShellBench, text: &str) -> Result<(f64, usize, usize), String> {
    let start = Instant::now();
    let tab = b.open(
        "search.run",
        &json!({ "text": text, "mode": "library", "split": true }),
    )?;
    let done = |b: &ShellBench| {
        let s = b.shell.session();
        s.search_tab(tab)
            .is_some_and(|t| !t.running && t.library_results.is_some())
    };
    b.wait("the Library search results", done)?;
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let hits = (b.shell.session().search_tab(tab))
        .and_then(|t| t.library_results.as_ref())
        .map_or(0, |r| r.rows.len());
    let searched = b
        .shell
        .session()
        .library
        .entries()
        .iter()
        .filter(|e| obs_engine::library_search::LIBRARY_TYPES.contains(&e.manifest.resource_type))
        .count();
    Ok((ms, searched, hits))
}

/// A Passage Guide opened on Romans 8:28, then moved to Romans 9:1: milliseconds until
/// every section is gathered, each time.
fn passage_guide(b: &mut ShellBench) -> Result<(f64, f64), String> {
    let gather = |b: &mut ShellBench, action: &dyn Fn(&mut ShellBench) -> Result<(), String>| {
        let start = Instant::now();
        action(b)?;
        // The sections start on the next frame.
        b.frame(None, |_| {});
        b.wait("every Passage Guide section", |b| {
            !b.shell.session().guides.is_busy()
        })?;
        Ok::<f64, String>(start.elapsed().as_secs_f64() * 1000.0)
    };
    let opened = gather(b, &|b| {
        b.open(
            "guide.passage_guide",
            &json!({ "passage": "Rom 8:28", "split": true }),
        )
        .map(|_| ())
    })?;
    let guide = b
        .tab_of(&TabKind::Guide(obs_engine::Guide::PassageGuide))
        .ok_or("no Passage Guide Tab")?;
    let moved = gather(b, &|b| {
        b.shell
            .run("tab.go", &json!({ "tab": guide.0, "text": "Rom 9:1" }))
            .map(|_| ())
            .ok_or_else(|| "the Passage Guide did not move".to_string())
    })?;
    Ok((opened, moved))
}

/// Open Bibles in new Panels until [`PANELS`] are open, and let them lay out.
fn open_panels(b: &mut ShellBench) -> Result<usize, String> {
    let bibles: Vec<String> = b
        .shell
        .session()
        .library
        .of_type(ResourceType::Bible)
        .skip(1)
        .take(PANELS)
        .map(|e| e.manifest.resource_id.clone())
        .collect();
    for id in bibles {
        if panels(b) >= PANELS {
            break;
        }
        b.open("resource.open", &json!({ "id": id, "split": true }))?;
        b.frame(None, |_| {});
    }
    let settle = Instant::now();
    while settle.elapsed() < Duration::from_secs(2) {
        b.frame(None, |_| {});
    }
    Ok(panels(b))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn the_mix_is_mostly_bibles_and_the_first_is_a_whole_bible() {
        let kinds: Vec<Template> = (0..SCALE_RESOURCES).map(kind_of).collect();
        assert_eq!(kinds[0], Template::WholeBible);
        let count = |k| kinds.iter().filter(|t| **t == k).count();
        assert_eq!(
            count(Template::WholeBible) + count(Template::NewTestament),
            1_760
        );
        assert_eq!(count(Template::Commentary), 120);
        assert_eq!(count(Template::Dictionary), 80);
        assert_eq!(count(Template::Lexicon), 40);
    }
}
