//! `cargo xtask tvtms <TVTMS.txt>`: regenerate the reformatted TVTMS rules that the engine
//! embeds (`crates/formats/assets/tvtms/tvtms-canon.tsv`).
//!
//! Input: the STEPBible-Data file `Versification/TVTMS - Translators Versification
//! Traditions with Methodology for Standardisation for Eng+Heb+Lat+Grk+Others -
//! STEPBible.org CC BY.txt` (CC BY 4.0). Kept: rows of the expanded data section whose
//! SourceRef is in a book of the 66-book canon, and only the columns SourceType, SourceRef,
//! StandardRef, Action and Tests. Notes, note markers and ancient-version columns are dropped.

use std::path::Path;

use anyhow::{Context, Result, bail};

const OUT: &str = "crates/formats/assets/tvtms/tvtms-canon.tsv";

const CANON: &[&str] = &[
    "GEN", "EXO", "LEV", "NUM", "DEU", "JOS", "JDG", "RUT", "1SA", "2SA", "1KI", "2KI", "1CH",
    "2CH", "EZR", "NEH", "EST", "JOB", "PSA", "PRO", "ECC", "SNG", "ISA", "JER", "LAM", "EZK",
    "DAN", "HOS", "JOL", "AMO", "OBA", "JON", "MIC", "NAM", "HAB", "ZEP", "HAG", "ZEC", "MAL",
    "MAT", "MRK", "LUK", "JHN", "ACT", "ROM", "1CO", "2CO", "GAL", "EPH", "PHP", "COL", "1TH",
    "2TH", "1TI", "2TI", "TIT", "PHM", "HEB", "JAS", "1PE", "2PE", "1JN", "2JN", "3JN", "JUD",
    "REV",
];

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let [input] = args else {
        bail!("usage: cargo xtask tvtms <path to the STEPBible TVTMS .txt>");
    };
    let raw = std::fs::read_to_string(input).with_context(|| format!("cannot read {input}"))?;
    let out = convert(&raw)?;
    let path = root.join(OUT);
    std::fs::write(&path, &out).with_context(|| format!("cannot write {}", path.display()))?;
    eprintln!(
        "tvtms: wrote {} rules ({} bytes) to {OUT}",
        out.lines().filter(|l| !l.starts_with('#')).count(),
        out.len()
    );
    Ok(())
}

fn convert(raw: &str) -> Result<String> {
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.starts_with("#DataStart(Expanded)"))
        .context("no #DataStart(Expanded) marker")?;
    let end = lines
        .iter()
        .position(|l| l.starts_with("#DataEnd(Expanded)"))
        .context("no #DataEnd(Expanded) marker")?;
    let mut out = String::from(
        "# Reformatted from STEPBible TVTMS (Translators Versification Traditions with Methodology\n\
         # for Standardisation), STEP Bible, https://www.STEPBible.org, licensed CC BY 4.0.\n\
         # Changes: only rows whose SourceRef is in the 66-book canon; only the columns SourceType,\n\
         # SourceRef, StandardRef, Action and Tests; whitespace trimmed. Regenerate with\n\
         # `cargo xtask tvtms <TVTMS.txt>`. Do not redistribute this file on its own.\n\
         # SourceType\tSourceRef\tStandardRef\tAction\tTests\n",
    );
    for line in &lines[start + 1..end] {
        let cols: Vec<&str> = line.split('\t').map(str::trim).collect();
        if cols.len() < 9 || cols[1].is_empty() || cols[0] == "SourceType" {
            continue;
        }
        let book = cols[1].get(..3).unwrap_or("").to_ascii_uppercase();
        if !CANON.contains(&book.as_str()) {
            continue;
        }
        out.push_str(&[cols[0], cols[1], cols[2], cols[3], cols[8]].join("\t"));
        out.push('\n');
    }
    Ok(out)
}
