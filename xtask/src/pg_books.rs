//! `cargo xtask pg-books`: the Project Gutenberg collection of the Open catalogue (#345),
//! `crates/engine/data/collections/pg-books.tsv`.
//!
//! Each row pins one eBook's posted HTML (`<n>-h/<n>-h.htm`), else its plain text, on PG's
//! mirror
//! (`obs_engine::open_catalogue::PG_MIRROR`) by its sha256, with its language, PG's catalogue
//! authors, the latest year any of them died, and its title. A book is added only when
//! - PG's catalogue (`pg_catalog.csv`) lists it as a Text, with a death year for every person
//!   it names, none later than `PG_LAST_DEATH_YEAR`;
//! - it is not marked "COPYRIGHTED Project Gutenberg eBook";
//! - its text does not name ccel.org or the Christian Classics Ethereal Library (#20: CCEL's
//!   transcriptions are never used, whoever hosts them);
//! - it imports as a Book (`obs_formats::import_pg_html`) with at least one Division.
//!
//! Usage:
//! - `cargo xtask pg-books --add numbers.txt`: add eBook numbers (one per line, `#` comments);
//! - `cargo xtask pg-books --refresh`: download every row's file again and re-pin it;
//! - `cargo xtask pg-books`: check every row again from the cache.
//!
//! The catalogue (`--catalog`, default: downloaded from the mirror into the cache) and the
//! files are cached under `--cache` (default `target/pg-books`). Downloads use `curl`, one at a
//! time with a pause, as PG asks of programs. The output is sorted by number.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use obs_engine::open_catalogue::{
    PG_BOOKS_HEADER, PG_LAST_DEATH_YEAR, PG_MIRROR, PgBook, parse_pg_books,
};

const OUT: &str = "crates/engine/data/collections/pg-books.tsv";
/// A title is cut, at a word, to about this many characters.
const MAX_TITLE: usize = 100;

struct Args {
    add: Option<PathBuf>,
    refresh: bool,
    cache: PathBuf,
    catalog: Option<PathBuf>,
}

fn parse_args(root: &Path, rest: &[String]) -> Result<Args> {
    let mut args = Args {
        add: None,
        refresh: false,
        cache: root.join("target/pg-books"),
        catalog: None,
    };
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--add" => args.add = Some(PathBuf::from(it.next().context("--add needs a file")?)),
            "--refresh" => args.refresh = true,
            "--cache" => args.cache = PathBuf::from(it.next().context("--cache needs a folder")?),
            "--catalog" => {
                args.catalog = Some(PathBuf::from(it.next().context("--catalog needs a file")?));
            }
            other => bail!("unknown argument {other:?}"),
        }
    }
    Ok(args)
}

fn curl(url: &str, to: &Path) -> Result<()> {
    let partial = to.with_extension("partial");
    let status = Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error"])
        .args([
            "--retry",
            "3",
            "--retry-delay",
            "10",
            "--connect-timeout",
            "30",
        ])
        .arg("--output")
        .arg(&partial)
        .arg(url)
        .status()
        .context("could not run `curl`")?;
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        bail!("download of {url} failed ({status})");
    }
    std::fs::rename(&partial, to)?;
    // PG asks programs to be gentle.
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(())
}

fn get_text(url: &str) -> Result<String> {
    let out = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--retry",
            "3",
        ])
        .arg(url)
        .output()
        .context("could not run `curl`")?;
    if !out.status.success() {
        bail!("{url}: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// RFC 4180 CSV: fields separated by commas, quoted with `"` (doubled inside), quoted
/// fields may hold commas and line breaks.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, '\r') => {}
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// One eBook as PG's catalogue lists it.
struct CatalogRow {
    kind: String,
    title: String,
    language: String,
    authors: String,
}

fn read_catalog(path: &Path) -> Result<BTreeMap<u32, CatalogRow>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let rows = parse_csv(&text);
    let header = rows.first().context("empty catalogue")?;
    let col = |name: &str| {
        header
            .iter()
            .position(|h| h == name)
            .with_context(|| format!("the catalogue has no {name} column"))
    };
    let (num, kind, title, lang, authors) = (
        col("Text#")?,
        col("Type")?,
        col("Title")?,
        col("Language")?,
        col("Authors")?,
    );
    let mut out = BTreeMap::new();
    for r in &rows[1..] {
        let get = |i: usize| r.get(i).cloned().unwrap_or_default();
        if let Ok(n) = get(num).parse::<u32>() {
            out.insert(
                n,
                CatalogRow {
                    kind: get(kind),
                    title: get(title),
                    language: get(lang),
                    authors: get(authors),
                },
            );
        }
    }
    Ok(out)
}

/// The latest death year of everyone PG names (`"Moody, Dwight Lyman, 1837-1899; Smith, A.,
/// 1801-1870 [Translator]"` → 1870 … 1899), or why it is not known.
fn latest_death(authors: &str) -> std::result::Result<u16, String> {
    let mut latest = 0u16;
    for person in authors.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let name = person.split('[').next().unwrap_or(person);
        // The last "birth-death" or "-death" span: its year after the hyphen.
        let died = name
            .rsplit_once('-')
            .map(|(_, after)| {
                after
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
            })
            .filter(|y| (3..=4).contains(&y.len()))
            .and_then(|y| y.parse::<u16>().ok());
        match died {
            Some(y) if !name.contains("BCE") || y > 0 => latest = latest.max(y),
            _ => return Err(format!("no death year for {person:?}")),
        }
    }
    if latest == 0 {
        return Err("no one named".into());
    }
    Ok(latest)
}

/// A catalogue title as a short title: its first line, cut at a colon or semicolon, at a
/// word to about [`MAX_TITLE`] characters.
fn short_title(title: &str) -> String {
    let first = title.lines().next().unwrap_or(title);
    let short = first
        .find([':', ';'])
        .map_or(first, |i| &first[..i])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let short = short.trim_end_matches([',', '.']);
    if short.chars().count() <= MAX_TITLE {
        return short.to_string();
    }
    let cut: String = short.chars().take(MAX_TITLE).collect();
    let at = cut.rfind(' ').unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end_matches([',', ';', ' ']))
}

/// The posted file's path under the mirror, from its folder's listing: its HTML
/// (`<n>-h/<n>-h.htm`), else its plain text (`<n>-0.txt`, UTF-8; `<n>.txt`; `<n>-8.txt`).
fn posted_file(number: u32) -> Result<String> {
    let folder = PgBook::folder(number);
    let listing = get_text(&format!("{PG_MIRROR}{folder}/"))?;
    let has = |name: &str| listing.contains(&format!("href=\"{name}\""));
    if has(&format!("{number}-h/")) {
        let dir = format!("{folder}/{number}-h/");
        let inner = get_text(&format!("{PG_MIRROR}{dir}"))?;
        for name in [format!("{number}-h.htm"), format!("{number}-h.html")] {
            if inner.contains(&format!("href=\"{name}\"")) {
                return Ok(format!("{dir}{name}"));
            }
        }
    }
    for name in [
        format!("{number}-0.txt"),
        format!("{number}.txt"),
        format!("{number}-8.txt"),
    ] {
        if has(&name) {
            return Ok(format!("{folder}/{name}"));
        }
    }
    bail!("no posted HTML or text in {PG_MIRROR}{folder}/")
}

fn check(number: u32, path: &str, bytes: &[u8], cat: &CatalogRow) -> Result<PgBook> {
    if cat.kind != "Text" {
        bail!("not a Text ({:?})", cat.kind);
    }
    let died = latest_death(&cat.authors).map_err(|e| anyhow::anyhow!(e))?;
    if died > PG_LAST_DEATH_YEAR {
        bail!("a contributor died in {died}, after {PG_LAST_DEATH_YEAR}");
    }
    let html = obs_formats::decode_html_bytes(bytes);
    let head = html
        .find("*** START OF")
        .or_else(|| html.find("***START OF"))
        .map_or(html.as_str(), |i| &html[..i]);
    if head.contains("COPYRIGHTED") {
        bail!("PG marks it copyrighted");
    }
    // A transcription made for the Christian Classics Ethereal Library is never used (#20).
    let lower = html.to_lowercase();
    if lower.contains("ccel.org") || lower.contains("christian classics ethereal") {
        bail!("its text came from the Christian Classics Ethereal Library");
    }
    let language = cat
        .language
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let title = short_title(&cat.title);
    let options = obs_formats::HtmlBookOptions {
        resource_id: "pg-check".into(),
        title: title.clone(),
        source_file: format!("{number}-h.htm"),
        tool_version: "xtask".into(),
        ..obs_formats::HtmlBookOptions::default()
    };
    let imported = if path.ends_with(".txt") {
        obs_formats::import_pg_text(&html, &options, |_, _| true)
    } else {
        obs_formats::import_pg_html(&html, &options, |_, _| true)
    };
    let (_, report) = imported.map_err(|e| anyhow::anyhow!("does not import: {e}"))?;
    if report.entries == 0 {
        bail!("no Divisions");
    }
    Ok(PgBook {
        number,
        path: path.to_string(),
        sha256: obs_engine::starter::sha256_hex(bytes),
        bytes: bytes.len() as u64,
        language: if language.is_empty() {
            "en".into()
        } else {
            language
        },
        authors: cat.authors.clone(),
        died,
        title,
    })
}

pub fn run(root: &Path, rest: &[String]) -> Result<()> {
    let args = parse_args(root, rest)?;
    std::fs::create_dir_all(&args.cache)?;
    let catalog_path = match &args.catalog {
        Some(p) => p.clone(),
        None => {
            let p = args.cache.join("pg_catalog.csv");
            if !p.exists() {
                curl(&format!("{PG_MIRROR}cache/epub/feeds/pg_catalog.csv"), &p)?;
            }
            p
        }
    };
    let catalog = read_catalog(&catalog_path)?;
    let out_path = root.join(OUT);
    let existing = match std::fs::read_to_string(&out_path) {
        Ok(text) => parse_pg_books(&text).map_err(|e| anyhow::anyhow!(e))?,
        Err(_) => Vec::new(),
    };
    let mut want: BTreeMap<u32, Option<PgBook>> =
        existing.into_iter().map(|b| (b.number, Some(b))).collect();
    if let Some(file) = &args.add {
        let text = std::fs::read_to_string(file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        for line in text.lines() {
            let n = line.split('#').next().unwrap_or_default().trim();
            if n.is_empty() {
                continue;
            }
            let n: u32 = n
                .parse()
                .with_context(|| format!("{}: {n:?} is not an eBook number", file.display()))?;
            want.entry(n).or_insert(None);
        }
    }
    let mut rows = Vec::new();
    let mut refused = Vec::new();
    let total = want.len();
    for (i, (number, row)) in want.into_iter().enumerate() {
        let result = (|| -> Result<PgBook> {
            let cat = catalog.get(&number).context("not in PG's catalogue")?;
            let path = match (&row, args.refresh) {
                (Some(r), false) => r.path.clone(),
                _ => posted_file(number)?,
            };
            let file = path.rsplit('/').next().unwrap_or(&path);
            let cached = args.cache.join(file);
            if args.refresh || !cached.exists() {
                curl(&format!("{PG_MIRROR}{path}"), &cached)?;
            }
            let bytes = std::fs::read(&cached)?;
            let new = check(number, &path, &bytes, cat)?;
            if let (Some(r), false) = (&row, args.refresh)
                && r.sha256 != new.sha256
            {
                bail!(
                    "the cached file is not the pinned one (sha256 {}, pinned {}); \
                     run with --refresh to re-pin",
                    new.sha256,
                    r.sha256
                );
            }
            Ok(new)
        })();
        match result {
            Ok(r) => rows.push(r),
            Err(e) => refused.push((number, format!("{e:#}"))),
        }
        if (i + 1).is_multiple_of(50) {
            eprintln!("pg-books: {} of {total}", i + 1);
        }
    }
    rows.sort_by_key(|r| r.number);
    let mut text = String::from(PG_BOOKS_HEADER);
    for row in &rows {
        text.push_str(&row.to_tsv());
        text.push('\n');
    }
    std::fs::write(&out_path, text)?;
    eprintln!(
        "pg-books: {} books in {OUT}; {} left out",
        rows.len(),
        refused.len()
    );
    for (n, why) in &refused {
        eprintln!("  left out {n}: {why}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn death_years_are_read_from_every_contributor() {
        assert_eq!(latest_death("Moody, Dwight Lyman, 1837-1899"), Ok(1899));
        assert_eq!(
            latest_death("Augustine, Saint, 354-430; Pusey, E. B., 1800-1882 [Translator]"),
            Ok(1882)
        );
        assert_eq!(latest_death("Hooper, John, -1555"), Ok(1555));
        assert!(latest_death("Anonymous").is_err());
        assert!(latest_death("Smith, John, 1880-").is_err());
        assert!(latest_death("Moody, D. L., 1837-1899; Various").is_err());
    }

    #[test]
    fn csv_quotes_hold_commas_and_line_breaks() {
        let rows = parse_csv("a,b\n1,\"x, \"\"y\"\"\nz\"\n");
        assert_eq!(rows, [vec!["a", "b"], vec!["1", "x, \"y\"\nz"]]);
    }

    #[test]
    fn catalogue_titles_are_cut_short() {
        assert_eq!(short_title("All of Grace\nAn Earnest Word"), "All of Grace");
        assert_eq!(short_title("Humility: The Beauty of Holiness"), "Humility");
    }
}
