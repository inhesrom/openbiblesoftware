//! `obs-cli index` and `obs-cli search`: build Search index caches (#58) and time queries
//! against them, the way the engine runs a search (open the cache, parse and compile the
//! query, collect every hit).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use obs_search::{Compiled, HitOrder, INDEX_VERSION, SearchIndex, SearchOptions, to_fts5};
use obs_store::{IndexKey, ObrReader, ResourceStore, SqliteSearchIndex, build_search_index};

const INDEX_USAGE: &str = "usage: obs-cli index <file.obr>... [--out DIR] [--version N]

  Build each Resource's Search index cache in DIR (default: the app's Search cache folder,
  $OBS_SEARCH_CACHE or the platform cache folder). Prints documents, size and build time.";

const SEARCH_USAGE: &str =
    "usage: obs-cli search <file.obr> [query] [--cache DIR] [--repeat N] [--show N] [--relevance]

  Run a query against the Resource's Search index cache (built first if missing) and print
  the hit count and timings. Each run opens the cache, parses and compiles the query and
  collects every hit, as the engine does. Without a query, run the timing set for the
  whole-Bible targets (#19): words and Strong's <= 100 ms, phrases <= 250 ms; the exit status
  is non-zero if a target is missed (measure a release build).";

/// The timing set: (query, kind, target).
const TIMING_SET: &[(&str, &str, u64)] = &[
    ("love", "word", 100),
    ("faith", "word", 100),
    ("the", "word", 100),
    ("lord god", "words", 100),
    ("lov*", "prefix", 100),
    ("faith -works", "words", 100),
    ("strong:G26", "strong", 100),
    ("H430", "strong", 100),
    ("strong:H1254", "strong", 100),
    ("strong:G5485 favour", "strong + word", 100),
    ("\"in the beginning\"", "phrase", 250),
    ("\"the son of man\"", "phrase", 250),
    ("\"and it came to pass\"", "phrase", 250),
    ("\"the lord\"", "phrase", 250),
];

fn cache_dir(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    explicit
        .or_else(obs_engine::default_search_cache_dir)
        .ok_or_else(|| "no Search cache folder; pass --out/--cache DIR".to_string())
}

pub fn index(args: &[String]) -> ExitCode {
    let mut files = Vec::new();
    let mut out = None;
    let mut version = INDEX_VERSION;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" | "-o" => out = it.next().map(PathBuf::from),
            "--version" => match it.next().and_then(|v| v.parse().ok()) {
                Some(v) => version = v,
                None => {
                    eprintln!("obs-cli index: `--version` needs a number\n{INDEX_USAGE}");
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{INDEX_USAGE}");
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with("--") => {
                eprintln!("obs-cli index: unknown option `{flag}`\n{INDEX_USAGE}");
                return ExitCode::from(2);
            }
            file => files.push(PathBuf::from(file)),
        }
    }
    if files.is_empty() {
        eprintln!("{INDEX_USAGE}");
        return ExitCode::from(2);
    }
    let dir = match cache_dir(out) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("obs-cli index: {e}");
            return ExitCode::from(2);
        }
    };
    let mut failed = false;
    for file in &files {
        match build_one(file, &dir, version) {
            Ok((path, docs, took)) => {
                let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
                println!(
                    "{}  {docs} documents  {:.1} MB  {:.2} s  {}",
                    file.display(),
                    bytes as f64 / 1_000_000.0,
                    took.as_secs_f64(),
                    path.display()
                );
            }
            Err(e) => {
                eprintln!("obs-cli index: {}: {e}", file.display());
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn build_one(file: &Path, dir: &Path, version: u32) -> Result<(PathBuf, u64, Duration), String> {
    let started = Instant::now();
    let resource = ObrReader::open(file)
        .and_then(|r| r.load())
        .map_err(|e| e.to_string())?;
    let path =
        build_search_index(&resource, version, dir, &mut |_, _| true).map_err(|e| e.to_string())?;
    let docs = SqliteSearchIndex::open(&path, None)
        .map_err(|e| e.to_string())?
        .document_count();
    Ok((path, docs, started.elapsed()))
}

struct SearchArgs {
    file: PathBuf,
    query: Option<String>,
    cache: Option<PathBuf>,
    repeat: usize,
    show: usize,
    order: HitOrder,
}

fn parse_search(args: &[String]) -> Result<SearchArgs, String> {
    let mut file = None;
    let mut words: Vec<&str> = Vec::new();
    let mut a = SearchArgs {
        file: PathBuf::new(),
        query: None,
        cache: None,
        repeat: 20,
        show: 5,
        order: HitOrder::Document,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut number = |name: &str| -> Result<usize, String> {
            it.next()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| format!("`{name}` needs a number"))
        };
        match arg.as_str() {
            "--cache" => a.cache = it.next().map(PathBuf::from),
            "--repeat" => a.repeat = number("--repeat")?.max(1),
            "--show" => a.show = number("--show")?,
            "--relevance" => a.order = HitOrder::Relevance,
            flag if flag.starts_with("--") => return Err(format!("unknown option `{flag}`")),
            other if file.is_none() => file = Some(PathBuf::from(other)),
            other => words.push(other),
        }
    }
    a.file = file.ok_or("missing <file.obr>")?;
    if !words.is_empty() {
        a.query = Some(words.join(" "));
    }
    Ok(a)
}

/// One timed query: hits and the sorted per-run times.
struct Timed {
    hits: usize,
    times: Vec<Duration>,
}

impl Timed {
    fn median(&self) -> Duration {
        self.times[self.times.len() / 2]
    }
    fn max(&self) -> Duration {
        self.times.last().copied().unwrap_or_default()
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Run `query` `repeat` times as the engine does; return the hits of the last run.
fn time_query(
    path: &Path,
    key: &IndexKey,
    lexicons: &[PathBuf],
    query: &str,
    repeat: usize,
    order: HitOrder,
) -> Result<(Timed, Vec<obs_search::SearchHit>), String> {
    let lookup = |t: &str| -> Vec<String> {
        lexicons
            .iter()
            .filter_map(|p| ObrReader::open(p).ok())
            .filter_map(|r| r.lemmas_for_transliteration(t).ok())
            .flatten()
            .collect()
    };
    let options = SearchOptions {
        order,
        ..Default::default()
    };
    let mut times = Vec::with_capacity(repeat);
    let mut hits = Vec::new();
    for _ in 0..repeat {
        let started = Instant::now();
        let index = SqliteSearchIndex::open(path, Some(key)).map_err(|e| e.to_string())?;
        let compiled = to_fts5(query, &lookup).map_err(|e| e.to_string())?;
        hits = match compiled {
            Compiled::Nothing => Vec::new(),
            c => index.search(&c, &options).map_err(|e| e.to_string())?,
        };
        times.push(started.elapsed());
    }
    times.sort();
    Ok((
        Timed {
            hits: hits.len(),
            times,
        },
        hits,
    ))
}

pub fn search(args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{SEARCH_USAGE}");
        return ExitCode::SUCCESS;
    }
    let a = match parse_search(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("obs-cli search: {e}\n{SEARCH_USAGE}");
            return ExitCode::from(2);
        }
    };
    match run_search(&a) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("obs-cli search: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Returns whether every timing target was met.
fn run_search(a: &SearchArgs) -> Result<bool, String> {
    let dir = cache_dir(a.cache.clone())?;
    let reader = ObrReader::open(&a.file).map_err(|e| format!("{}: {e}", a.file.display()))?;
    let manifest = reader.manifest().clone();
    drop(reader);
    let key = IndexKey::new(&manifest, INDEX_VERSION);
    let path = dir.join(key.file_name());
    if !path.is_file() {
        eprintln!("building the Search index in {} …", dir.display());
        build_one(&a.file, &dir, INDEX_VERSION)?;
    }
    // Lexicons beside the file answer `lemma:` transliterations.
    let lexicons: Vec<PathBuf> = a
        .file
        .parent()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "obr"))
                .filter(|p| {
                    ObrReader::open(p).is_ok_and(|r| {
                        r.manifest().resource_type == obs_model::ResourceType::Lexicon
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let documents = SqliteSearchIndex::open(&path, Some(&key))
        .map_err(|e| e.to_string())?
        .document_count();
    println!(
        "{} ({}): {documents} documents, index {}",
        manifest.abbreviation,
        manifest.resource_id,
        path.display()
    );
    if let Some(q) = &a.query {
        let (t, hits) = time_query(&path, &key, &lexicons, q, a.repeat, a.order)?;
        println!(
            "{q}: {} hits  median {:.1} ms  min {:.1} ms  max {:.1} ms  ({} runs)",
            t.hits,
            ms(t.median()),
            ms(t.times[0]),
            ms(t.max()),
            t.times.len()
        );
        for h in hits.iter().take(a.show) {
            let key = h.key.as_ref().map(ToString::to_string).unwrap_or_default();
            println!(
                "  #{:<6} {:<24} {} {}..{}",
                h.document, key, h.unit, h.range.start, h.range.end
            );
        }
        return Ok(true);
    }
    println!(
        "{:<26} {:<14} {:>7} {:>9} {:>9} {:>8}",
        "query", "kind", "hits", "median", "max", "target"
    );
    let mut all_met = true;
    for (q, kind, target) in TIMING_SET {
        let (t, _) = time_query(&path, &key, &lexicons, q, a.repeat, a.order)?;
        let met = t.median() <= Duration::from_millis(*target);
        all_met &= met;
        println!(
            "{:<26} {:<14} {:>7} {:>6.1} ms {:>6.1} ms {:>5} ms {}",
            q,
            kind,
            t.hits,
            ms(t.median()),
            ms(t.max()),
            target,
            if met { "ok" } else { "MISSED" }
        );
    }
    if cfg!(debug_assertions) {
        println!("(debug build: measure the targets with a release build)");
    }
    Ok(all_met)
}
