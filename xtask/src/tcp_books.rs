//! `cargo xtask tcp-books`: the Text Creation Partnership collection of the Open catalogue
//! (#345), `crates/engine/data/collections/tcp-books.tsv`.
//!
//! Each row pins one TCP text: its repository's commit and the file's sha256, with what its
//! TEI header says (language, date, author, title) and which of TCP's CC0 statements it
//! carries. A text is added only when
//! - its `<availability>` is, whitespace aside, one of the statements in
//!   `obs_engine::open_catalogue::TCP_CC0_STATEMENTS` (quoted in `open-evidence/tcp.md`),
//! - and it imports as a Book with at least one Division.
//!
//! Usage:
//! - `cargo xtask tcp-books --add ids.txt`: add the TCP ids listed (one per line, `#`
//!   comments), pinned at each repository's current commit;
//! - `cargo xtask tcp-books --refresh`: re-pin every row at its repository's current commit;
//! - `cargo xtask tcp-books`: check every row again from the cache (or a fresh download).
//!
//! Downloads are cached under `<cache>/<id>-<commit>.xml` (`--cache`, default
//! `target/tcp-books`). Repositories are read with `git ls-remote`, files with `curl`. The
//! output is sorted by id, so the same inputs give the same bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use obs_engine::open_catalogue::{
    TCP_BOOKS_HEADER, TcpBook, open_catalogue_toml_sources, parse_tcp_books, tcp_statement,
};

const OUT: &str = "crates/engine/data/collections/tcp-books.tsv";

struct Args {
    add: Option<PathBuf>,
    refresh: bool,
    cache: PathBuf,
    jobs: usize,
}

fn parse_args(root: &Path, rest: &[String]) -> Result<Args> {
    let mut args = Args {
        add: None,
        refresh: false,
        cache: root.join("target/tcp-books"),
        jobs: 8,
    };
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--add" => args.add = Some(PathBuf::from(it.next().context("--add needs a file")?)),
            "--refresh" => args.refresh = true,
            "--cache" => {
                args.cache = PathBuf::from(it.next().context("--cache needs a folder")?);
            }
            "--jobs" => {
                args.jobs = it.next().context("--jobs needs a number")?.parse()?;
            }
            other => bail!("unknown argument {other:?}"),
        }
    }
    Ok(args)
}

/// A TCP id: a capital letter, digits, and an optional `.NNN` part (`A53715`, `K070362.001`).
fn is_tcp_id(id: &str) -> bool {
    let mut parts = id.splitn(2, '.');
    let main = parts.next().unwrap_or_default();
    let part_ok = parts
        .next()
        .is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    main.len() >= 5
        && main.as_bytes()[0].is_ascii_uppercase()
        && main[1..].bytes().all(|b| b.is_ascii_digit())
        && part_ok
}

/// The repository's current commit.
fn head_commit(id: &str) -> Result<String> {
    let url = format!("https://github.com/textcreationpartnership/{id}");
    for attempt in 0..4 {
        let out = Command::new("git")
            .args(["ls-remote", &url, "HEAD"])
            .output()
            .context("could not run `git`")?;
        if out.status.success()
            && let Some(sha) = String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .next()
            && sha.len() == 40
        {
            return Ok(sha.to_string());
        }
        std::thread::sleep(std::time::Duration::from_secs(2 + attempt * 5));
    }
    bail!("{id}: no repository at {url}")
}

/// The text at `commit`, from the cache or downloaded.
fn text_at(id: &str, commit: &str, cache: &Path) -> Result<Vec<u8>> {
    std::fs::create_dir_all(cache)?;
    let path = cache.join(format!("{id}-{commit}.xml"));
    if let Ok(bytes) = std::fs::read(&path) {
        return Ok(bytes);
    }
    let url = TcpBook::raw_url(id, commit);
    let partial = path.with_extension("partial");
    let status = Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error"])
        .args([
            "--retry",
            "3",
            "--retry-delay",
            "5",
            "--connect-timeout",
            "30",
        ])
        .arg("--output")
        .arg(&partial)
        .arg(&url)
        .status()
        .context("could not run `curl`")?;
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        bail!("{id}: download of {url} failed ({status})");
    }
    std::fs::rename(&partial, &path)?;
    Ok(std::fs::read(&path)?)
}

/// Check one text and make its row.
fn check(id: &str, commit: &str, bytes: &[u8]) -> Result<TcpBook> {
    let xml = std::str::from_utf8(bytes).with_context(|| format!("{id}: not UTF-8"))?;
    let header = obs_formats::read_tei_header(xml).map_err(|e| anyhow::anyhow!("{id}: {e}"))?;
    let statement = tcp_statement(&header.availability).with_context(|| {
        format!(
            "{id}: its <availability> is not one of TCP's CC0 statements: {:?}",
            header.availability
        )
    })?;
    if header.id != id {
        bail!("{id}: its header names {:?}", header.id);
    }
    let options = obs_formats::TeiBookOptions {
        resource_id: "tcp-check".into(),
        source_file: format!("{id}.xml"),
        tool_version: "xtask".into(),
        ..obs_formats::TeiBookOptions::default()
    };
    let (_, report) = obs_formats::import_tcp_tei(xml, &options, |_, _| true)
        .map_err(|e| anyhow::anyhow!("{id}: does not import: {e}"))?;
    if report.entries == 0 {
        bail!("{id}: no Divisions");
    }
    Ok(TcpBook {
        id: id.to_string(),
        commit: commit.to_string(),
        sha256: obs_engine::starter::sha256_hex(bytes),
        bytes: bytes.len() as u64,
        language: header.language,
        date: header.date,
        author: header.author,
        title: obs_formats::short_title(&header.title),
        statement: statement.to_string(),
    })
}

pub fn run(root: &Path, rest: &[String]) -> Result<()> {
    let args = parse_args(root, rest)?;
    let out_path = root.join(OUT);
    let existing = match std::fs::read_to_string(&out_path) {
        Ok(text) => parse_tcp_books(&text).map_err(|e| anyhow::anyhow!(e))?,
        Err(_) => Vec::new(),
    };
    // Texts the hand-written entries list already are left out of the collection.
    let listed: std::collections::BTreeSet<String> = open_catalogue_toml_sources()
        .iter()
        .filter_map(|s| TcpBook::id_of_url(&s.url))
        .collect();
    let mut want: BTreeMap<String, Option<String>> = existing
        .iter()
        .map(|b| (b.id.clone(), (!args.refresh).then(|| b.commit.clone())))
        .collect();
    if let Some(file) = &args.add {
        let text = std::fs::read_to_string(file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        for line in text.lines() {
            let id = line.split('#').next().unwrap_or_default().trim();
            if id.is_empty() {
                continue;
            }
            if !is_tcp_id(id) {
                bail!("{}: {id:?} is not a TCP id", file.display());
            }
            want.entry(id.to_string()).or_insert(None);
        }
    }
    want.retain(|id, _| !listed.contains(id));
    let jobs: Vec<(String, Option<String>)> = want.into_iter().collect();
    let total = jobs.len();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..args.jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((id, commit)) = jobs.get(i) else {
                        break;
                    };
                    let row = (|| {
                        let commit = match commit {
                            Some(c) => c.clone(),
                            None => head_commit(id)?,
                        };
                        let bytes = text_at(id, &commit, &args.cache)?;
                        check(id, &commit, &bytes)
                    })();
                    if (i + 1).is_multiple_of(250) {
                        eprintln!("tcp-books: {} of {total}", i + 1);
                    }
                    results
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((id.clone(), row));
                }
            });
        }
    });
    let mut rows = Vec::new();
    let mut refused = Vec::new();
    for (id, row) in results.into_inner().unwrap_or_else(|e| e.into_inner()) {
        match row {
            Ok(row) => rows.push(row),
            Err(e) => refused.push((id, format!("{e:#}"))),
        }
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    refused.sort();
    let mut text = String::from(TCP_BOOKS_HEADER);
    for row in &rows {
        text.push_str(&row.to_tsv());
        text.push('\n');
    }
    if let Some(dir) = out_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out_path, text)?;
    let mut by_statement: BTreeMap<&str, usize> = BTreeMap::new();
    for row in &rows {
        *by_statement.entry(row.statement.as_str()).or_default() += 1;
    }
    eprintln!(
        "tcp-books: {} texts in {OUT} ({}); {} left out",
        rows.len(),
        by_statement
            .iter()
            .map(|(k, n)| format!("{n} {k}"))
            .collect::<Vec<_>>()
            .join(", "),
        refused.len()
    );
    for (id, why) in &refused {
        eprintln!("  left out {id}: {why}");
    }
    Ok(())
}
