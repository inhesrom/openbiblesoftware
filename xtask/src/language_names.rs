//! `cargo xtask language-names`: regenerate the language names built into the app (#244),
//! `crates/engine/assets/languages/`, from Unicode CLDR and the ISO 639-3 code tables.
//!
//! Inputs, pinned by sha256 and cached with the Starter downloads:
//! - CLDR's locale display names, `cldr-localenames-full` from npm (the JSON the CLDR project
//!   publishes), Unicode License v3;
//! - CLDR's likely subtags, `cldr-core` from npm, for the script each language is usually
//!   written in (a tag naming that script, `bn-Beng-IN`, names the same language as `bn-IN`);
//! - SIL's ISO 639-3 code tables (a dated zip from iso639-3.sil.org), whose terms allow
//!   incorporating them into software with attribution to iso639-3.sil.org.
//!
//! Outputs (the engine's `language_names` module reads them):
//! - `cldr-names.tsv`: `L` rows (language code, English name, the name in the language itself,
//!   other English names), `S` rows (script code, English name), `R` rows (region code,
//!   English name) and `D` rows (language code, the script it is usually written in);
//! - `iso-639-3.tsv`: each ISO 639-3 code with its ISO 639-2/B and 639-1 codes and its
//!   reference name, unchanged, then each retired code with its reference name and the code
//!   that replaced it (none when it was split);
//! - `LICENSE-CLDR`: CLDR's licence, which must travel with the data.
//!
//! The output is deterministic: the same inputs give the same bytes.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::starter::{Config, PinnedFile, fetch_file};

const OUT_DIR: &str = "crates/engine/assets/languages";

const CLDR_VERSION: &str = "49.0.0";
const CLDR: (&str, &str) = (
    "https://registry.npmjs.org/cldr-localenames-full/-/cldr-localenames-full-49.0.0.tgz",
    "d00bafb534848f142b3645d6046be81a1094fb00e809cd2c519f5e031198ba11",
);
const CLDR_CORE: (&str, &str) = (
    "https://registry.npmjs.org/cldr-core/-/cldr-core-49.0.0.tgz",
    "085b1c7d83f00692505c79d575c21eea89534466eb3e70b455688851a45c87bc",
);
const ISO_TABLES: &str = "iso-639-3_Code_Tables_20261015";
const ISO: (&str, &str) = (
    "https://iso639-3.sil.org/sites/iso639-3/files/downloads/iso-639-3_Code_Tables_20261015.zip",
    "1b75148ff267404afd32d61464f636e692b5a15396418c9b728485237ee5eb37",
);

/// CLDR alternative English names kept as other names (a search finds them); `-alt-menu`
/// forms ("Kurdish, Central") are for sorted menus and left out.
const ALTS: [&str; 4] = ["-alt-variant", "-alt-short", "-alt-long", "-alt-official"];

const USAGE: &str = "usage: cargo xtask language-names [--cache <dir>]

Regenerates crates/engine/assets/languages/ from Unicode CLDR and the ISO 639-3 code tables
(downloads are pinned by sha256 and cached in target/starter-cache).";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let mut cache = Config::defaults(root).cache;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cache" => {
                cache = it.next().context("--cache needs a directory")?.into();
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => bail!("unknown option `{other}`\n\n{USAGE}"),
        }
    }
    let pinned = |(url, sha256): (&str, &str)| PinnedFile {
        url: url.to_string(),
        sha256: sha256.to_string(),
    };
    let cldr = fetch_file("language-names", &pinned(CLDR), &cache)?;
    let core = fetch_file("language-names", &pinned(CLDR_CORE), &cache)?;
    let iso = fetch_file("language-names", &pinned(ISO), &cache)?;
    let (locales, license) = cldr_locales(&cldr)?;
    let scripts = default_scripts(&likely_subtags(&core)?)?;
    let iso_tab = iso_table(&iso, "iso-639-3.tab")?;
    let retired = iso_table(&iso, "iso-639-3_Retirements.tab")?;
    let dir = root.join(OUT_DIR);
    std::fs::create_dir_all(&dir)?;
    let write = |name: &str, text: &str| -> Result<()> {
        let path = dir.join(name);
        std::fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))?;
        eprintln!(
            "language-names: wrote {} bytes to {OUT_DIR}/{name}",
            text.len()
        );
        Ok(())
    };
    write("cldr-names.tsv", &cldr_names(&locales, &scripts)?)?;
    write("iso-639-3.tsv", &iso_names(&iso_tab, &retired)?)?;
    write("LICENSE-CLDR", &license)?;
    Ok(())
}

/// One CLDR locale's display names: languages, scripts, regions.
#[derive(Debug, Default)]
struct Locale {
    languages: BTreeMap<String, String>,
    scripts: BTreeMap<String, String>,
    regions: BTreeMap<String, String>,
}

/// Every locale's display names from the npm tarball, and CLDR's licence text.
fn cldr_locales(tgz: &[u8]) -> Result<(BTreeMap<String, Locale>, String)> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(tgz)));
    let mut locales: BTreeMap<String, Locale> = BTreeMap::new();
    let mut license = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        let parts: Vec<&str> = path.split('/').collect();
        if parts == ["package", "LICENSE"] {
            let mut text = String::new();
            entry.read_to_string(&mut text)?;
            license = Some(text);
            continue;
        }
        let ["package", "main", locale, file] = parts[..] else {
            continue;
        };
        let key = match file {
            "languages.json" => "languages",
            "scripts.json" => "scripts",
            "territories.json" => "territories",
            _ => continue,
        };
        let mut text = String::new();
        entry.read_to_string(&mut text)?;
        let json: Value =
            serde_json::from_str(&text).with_context(|| format!("{path} is not JSON"))?;
        let names: BTreeMap<String, String> = json["main"][locale]["localeDisplayNames"][key]
            .as_object()
            .with_context(|| format!("{path}: no {key}"))?
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
            .collect();
        let l = locales.entry(locale.to_string()).or_default();
        match key {
            "languages" => l.languages = names,
            "scripts" => l.scripts = names,
            _ => l.regions = names,
        }
    }
    let license = license.context("the CLDR package has no LICENSE")?;
    Ok((locales, license))
}

/// `supplemental/likelySubtags.json` from the `cldr-core` tarball.
fn likely_subtags(tgz: &[u8]) -> Result<String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(tgz)));
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.path()?.to_string_lossy() == "package/supplemental/likelySubtags.json" {
            let mut text = String::new();
            entry.read_to_string(&mut text)?;
            return Ok(text);
        }
    }
    bail!("the cldr-core package has no supplemental/likelySubtags.json")
}

/// Each language's usual script, from CLDR's likely subtags: `bn` → `bn-Beng-BD` gives
/// `Beng`. Only bare language keys (`und-…` and keys with a script or region are left out).
fn default_scripts(likely: &str) -> Result<BTreeMap<String, String>> {
    let json: Value = serde_json::from_str(likely).context("likelySubtags.json is not JSON")?;
    let map = json["supplemental"]["likelySubtags"]
        .as_object()
        .context("likelySubtags.json: no supplemental.likelySubtags")?;
    Ok(map
        .iter()
        .filter(|(k, _)| !k.contains('-') && k.as_str() != "und")
        .filter_map(|(k, v)| {
            let script = v.as_str()?.split('-').nth(1)?;
            (script.len() == 4).then(|| (k.clone(), script.to_string()))
        })
        .collect())
}

/// The tags of a CLDR key without an `-alt-…` or `-menu-…` suffix.
fn plain(key: &str) -> bool {
    !key.contains("-alt-") && !key.contains("-menu-")
}

/// `{0} ({1})`, CLDR's English locale pattern, for a language with a script or region.
fn composed(base: &str, parts: &[&str]) -> String {
    if parts.is_empty() {
        base.to_string()
    } else {
        format!("{base} ({})", parts.join(", "))
    }
}

/// A locale id's name in its own language: the locale's own entry for itself, else its base
/// language's name with its script and region names, all in that locale.
fn native_name(id: &str, locale: &Locale) -> Option<String> {
    if let Some(n) = locale.languages.get(id) {
        return Some(n.clone());
    }
    let mut subtags = id.split('-');
    let base = locale.languages.get(subtags.next()?)?;
    let parts: Vec<&str> = subtags
        .map(|s| {
            if s.len() == 4 {
                locale.scripts.get(s)
            } else {
                locale.regions.get(s)
            }
            .map(String::as_str)
        })
        .collect::<Option<_>>()?;
    Some(composed(base, &parts))
}

fn tsv_field(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

/// `cldr-names.tsv`: CLDR's English name of every language (and of every locale that is a
/// language in a script, `sr-Latn`), with each one's name in its own language when CLDR has
/// that locale; then the English script and region names.
fn cldr_names(
    locales: &BTreeMap<String, Locale>,
    default_scripts: &BTreeMap<String, String>,
) -> Result<String> {
    let en = locales.get("en").context("CLDR has no `en` locale")?;
    let mut english: BTreeMap<String, String> = en
        .languages
        .iter()
        .filter(|(k, _)| plain(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    // Script locales (`sr-Latn`, `uz-Cyrl`): "Serbian (Latin)", as CLDR composes it.
    for id in locales.keys() {
        let subtags: Vec<&str> = id.split('-').collect();
        if let [lang, script] = subtags[..]
            && script.len() == 4
            && !english.contains_key(id)
            && let (Some(l), Some(s)) = (en.languages.get(lang), en.scripts.get(script))
        {
            english.insert(id.clone(), composed(l, &[s]));
        }
    }
    let mut out = format!(
        "# Language, script and region names from Unicode CLDR {CLDR_VERSION} \
         (cldr-localenames-full), Unicode License v3 (LICENSE-CLDR).\n\
         # Generated by `cargo xtask language-names`; do not edit.\n\
         # L<TAB>code<TAB>English name<TAB>name in the language itself<TAB>other English names (|)\n\
         # S<TAB>script code<TAB>English name\n\
         # R<TAB>region code<TAB>English name\n\
         # D<TAB>language code<TAB>the script it is usually written in (CLDR likely subtags, cldr-core)\n"
    );
    for (code, name) in &english {
        let native = locales
            .get(code)
            .and_then(|l| native_name(code, l))
            .filter(|n| !n.eq_ignore_ascii_case(name))
            .unwrap_or_default();
        let also: Vec<&str> = ALTS
            .iter()
            .filter_map(|alt| en.languages.get(&format!("{code}{alt}")))
            .map(String::as_str)
            .filter(|a| !a.eq_ignore_ascii_case(name))
            .collect();
        out.push_str(&format!(
            "L\t{code}\t{}\t{}\t{}\n",
            tsv_field(name),
            tsv_field(&native),
            tsv_field(&also.join("|"))
        ));
    }
    for (code, name) in en.scripts.iter().filter(|(k, _)| plain(k)) {
        out.push_str(&format!("S\t{code}\t{}\n", tsv_field(name)));
    }
    for (code, name) in en.regions.iter().filter(|(k, _)| plain(k)) {
        out.push_str(&format!("R\t{code}\t{}\n", tsv_field(name)));
    }
    for (code, script) in default_scripts {
        out.push_str(&format!("D\t{code}\t{script}\n"));
    }
    Ok(out)
}

/// One table (`iso-639-3.tab`, …) from the code tables zip.
fn iso_table(zip: &[u8], name: &str) -> Result<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(zip)).context("not a zip archive")?;
    let mut file = zip
        .by_name(&format!("{ISO_TABLES}/{name}"))
        .with_context(|| format!("the ISO 639-3 zip has no {name}"))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

/// A tab-separated table: its rows, each a map of column name to trimmed value.
fn tab_rows(tab: &str) -> Result<Vec<BTreeMap<&str, &str>>> {
    let mut lines = tab.lines();
    let header: Vec<&str> = lines
        .next()
        .context("empty table")?
        .trim_start_matches('\u{feff}')
        .split('\t')
        .map(str::trim)
        .collect();
    Ok(lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            header
                .iter()
                .copied()
                .zip(l.split('\t').map(str::trim))
                .collect()
        })
        .collect())
}

/// A column's value in a [`tab_rows`] row, empty when missing.
fn get<'a>(row: &BTreeMap<&str, &'a str>, column: &str) -> &'a str {
    row.get(column).copied().unwrap_or_default()
}

/// `iso-639-3.tsv`: Id, Part2b when it differs, Part1, Ref_Name for every code; then Id,
/// Ref_Name and Change_To for every retired code that is not a code again.
fn iso_names(tab: &str, retired: &str) -> Result<String> {
    let mut out = format!(
        "# ISO 639-3 codes and reference names from SIL International's code tables \
         ({ISO_TABLES}), https://iso639-3.sil.org/.\n\
         # Generated by `cargo xtask language-names`; do not edit.\n\
         # Id<TAB>Part2b (when it differs)<TAB>Part1<TAB>Ref_Name[<TAB>Change_To, for a retired code]\n"
    );
    let codes = tab_rows(tab).context("iso-639-3.tab")?;
    for r in &codes {
        let (id, b) = (get(r, "Id"), get(r, "Part2b"));
        if id.is_empty() {
            bail!("iso-639-3.tab: a row without an Id");
        }
        let b = if b == id { "" } else { b };
        out.push_str(&format!(
            "{id}\t{b}\t{}\t{}\n",
            get(r, "Part1"),
            tsv_field(get(r, "Ref_Name"))
        ));
    }
    let current: std::collections::BTreeSet<&str> = codes.iter().map(|r| get(r, "Id")).collect();
    for r in tab_rows(retired).context("iso-639-3_Retirements.tab")? {
        let id = get(&r, "Id");
        if id.is_empty() || current.contains(id) {
            continue;
        }
        out.push_str(&format!(
            "{id}\t\t\t{}\t{}\n",
            tsv_field(get(&r, "Ref_Name")),
            get(&r, "Change_To")
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locale(languages: &[(&str, &str)], scripts: &[(&str, &str)]) -> Locale {
        let map = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        };
        Locale {
            languages: map(languages),
            scripts: map(scripts),
            regions: BTreeMap::new(),
        }
    }

    #[test]
    fn cldr_rows_carry_the_native_name_and_alternatives_and_compose_script_locales() {
        let mut locales = BTreeMap::new();
        locales.insert(
            "en".to_string(),
            locale(
                &[
                    ("bn", "Bangla"),
                    ("sr", "Serbian"),
                    ("en", "English"),
                    ("ckb", "Central Kurdish"),
                    ("ckb-alt-variant", "Sorani Kurdish"),
                    ("ckb-alt-menu", "Kurdish, Central"),
                ],
                &[("Latn", "Latin")],
            ),
        );
        locales.insert("bn".to_string(), locale(&[("bn", "বাংলা")], &[]));
        locales.insert(
            "sr-Latn".to_string(),
            locale(&[("sr", "srpski")], &[("Latn", "latinica")]),
        );
        let scripts = default_scripts(
            r#"{"supplemental": {"likelySubtags": {
                "bn": "bn-Beng-BD", "und": "en-Latn-US", "und-Beng": "bn-Beng-BD",
                "sr-ME": "sr-Latn-ME", "sr": "sr-Cyrl-RS"}}}"#,
        )
        .unwrap();
        let out = cldr_names(&locales, &scripts).unwrap();
        assert!(out.contains("L\tbn\tBangla\tবাংলা\t\n"), "{out}");
        assert!(
            out.contains("L\ten\tEnglish\t\t\n"),
            "same name, no native: {out}"
        );
        assert!(out.contains("L\tckb\tCentral Kurdish\t\tSorani Kurdish\n"));
        assert!(!out.contains("ckb-alt"));
        assert!(out.contains("L\tsr-Latn\tSerbian (Latin)\tsrpski (latinica)\t\n"));
        assert!(out.contains("S\tLatn\tLatin\n"));
        let defaults: Vec<&str> = out.lines().filter(|l| l.starts_with("D\t")).collect();
        assert_eq!(defaults, ["D\tbn\tBeng", "D\tsr\tCyrl"]);
    }

    #[test]
    fn iso_rows_keep_the_codes_and_reference_name_and_retired_codes_follow() {
        let tab = "Id\tPart2b\tPart2t\tPart1\tScope\tLanguage_Type\tRef_Name\tComment\n\
                   ben\tben\tben\tbn\tI\tL\tBengali\t\n\
                   deu\tger\tdeu\tde\tI\tL\tGerman\t\n\
                   kmo\t\t\t\tI\tL\tKwoma\t\n";
        let retired = "Id\tRef_Name\tRet_Reason\tChange_To\tRet_Remedy\tEffective\n\
                       daf\tDan\tS\t\tSplit into Dan [dnj] and Kla-Dan [lda]\t2013-01-23\n\
                       mof\tMohegan-Montauk-Narragansett\tS\t\tSplit\t2010-01-26\n\
                       ggr\tAghu Tharnggalu\tD\tggm\t\t2013-01-23\n\
                       kmo\tKwoma\tD\tkmo\t\t2000-01-01\n";
        let out = iso_names(tab, retired).unwrap();
        let rows: Vec<&str> = out.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(
            rows,
            [
                "ben\t\tbn\tBengali",
                "deu\tger\tde\tGerman",
                "kmo\t\t\tKwoma",
                "daf\t\t\tDan\t",
                "mof\t\t\tMohegan-Montauk-Narragansett\t",
                "ggr\t\t\tAghu Tharnggalu\tggm",
            ]
        );
    }
}
