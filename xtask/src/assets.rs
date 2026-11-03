//! Asset attribution gate.
//!
//! Every file inside a directory named `assets` under `crates/` or `apps/` must match a
//! `path` glob of an `[[asset]]` entry in the root `ATTRIBUTION.toml`. Each entry must also
//! match at least one file, so stale rows are caught.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use globset::{Glob, GlobMatcher};
use serde::Deserialize;
use walkdir::WalkDir;

const FILE: &str = "ATTRIBUTION.toml";
const SCAN_ROOTS: &[&str] = &["crates", "apps"];
const ASSET_DIR: &str = "assets";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribution {
    #[serde(default)]
    pub asset: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// Glob relative to the workspace root, e.g. `crates/fonts/assets/gentium/*`.
    pub path: String,
    pub name: String,
    /// SPDX licence expression.
    pub license: String,
    pub source: String,
    pub copyright: String,
    /// Changes we made, if any (required by some licences, e.g. CC BY).
    #[serde(default)]
    pub changes: Option<String>,
}

pub fn run(root: &Path) -> Result<()> {
    let text =
        std::fs::read_to_string(root.join(FILE)).with_context(|| format!("cannot read {FILE}"))?;
    let attribution: Attribution =
        toml::from_str(&text).with_context(|| format!("cannot parse {FILE}"))?;
    let files = asset_files(root)?;
    let errors = check(&attribution, &files)?;
    if errors.is_empty() {
        eprintln!(
            "assets: {} file(s) covered by {} entr(y/ies)",
            files.len(),
            attribution.asset.len()
        );
        return Ok(());
    }
    for e in &errors {
        eprintln!("assets: {e}");
    }
    bail!("{} attribution problem(s); edit {FILE}", errors.len())
}

/// All files under any `assets` directory, relative to `root`, with `/` separators.
fn asset_files(root: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for scan in SCAN_ROOTS {
        let dir = root.join(scan);
        if !dir.exists() {
            continue;
        }
        for entry in WalkDir::new(&dir)
            .into_iter()
            .filter_entry(|e| e.file_name() != "target")
        {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel: PathBuf = entry.path().strip_prefix(root)?.to_path_buf();
            if rel.components().any(|c| c.as_os_str() == ASSET_DIR) {
                out.push(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Returns one message per uncovered file, per entry matching nothing, and per empty field.
pub fn check(attribution: &Attribution, files: &[String]) -> Result<Vec<String>> {
    let matchers: Vec<(&Asset, GlobMatcher)> = attribution
        .asset
        .iter()
        .map(|a| {
            Glob::new(&a.path)
                .map(|g| (a, g.compile_matcher()))
                .with_context(|| format!("bad glob `{}`", a.path))
        })
        .collect::<Result<_>>()?;
    let mut errors = Vec::new();
    for a in &attribution.asset {
        for (field, value) in [
            ("name", &a.name),
            ("license", &a.license),
            ("source", &a.source),
            ("copyright", &a.copyright),
        ] {
            if value.trim().is_empty() {
                errors.push(format!("entry `{}` has an empty `{field}`", a.path));
            }
        }
        if a.changes.as_deref().is_some_and(|c| c.trim().is_empty()) {
            errors.push(format!(
                "entry `{}` has an empty `changes`; omit it or describe the changes",
                a.path
            ));
        }
    }
    for f in files {
        if !matchers.iter().any(|(_, m)| m.is_match(f)) {
            errors.push(format!("`{f}` has no ATTRIBUTION.toml entry"));
        }
    }
    for (a, m) in &matchers {
        if !files.iter().any(|f| m.is_match(f)) {
            errors.push(format!("entry `{}` matches no asset file", a.path));
        }
    }
    Ok(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attribution(toml_text: &str) -> Attribution {
        toml::from_str(toml_text).unwrap()
    }

    const ONE: &str = r#"
        [[asset]]
        path = "crates/fonts/assets/gentium/*"
        name = "Gentium Plus"
        license = "OFL-1.1"
        source = "https://software.sil.org/gentium/"
        copyright = "SIL International"
    "#;

    #[test]
    fn covered_files_pass() {
        let files = vec!["crates/fonts/assets/gentium/GentiumPlus-Regular.ttf".to_string()];
        assert!(check(&attribution(ONE), &files).unwrap().is_empty());
    }

    #[test]
    fn unlisted_file_fails() {
        let files = vec![
            "crates/fonts/assets/gentium/GentiumPlus-Regular.ttf".to_string(),
            "crates/fonts/assets/ezra/SILEOT.ttf".to_string(),
        ];
        let errors = check(&attribution(ONE), &files).unwrap();
        assert_eq!(
            errors,
            vec!["`crates/fonts/assets/ezra/SILEOT.ttf` has no ATTRIBUTION.toml entry"]
        );
    }

    #[test]
    fn stale_entry_fails() {
        let errors = check(&attribution(ONE), &[]).unwrap();
        assert!(errors[0].contains("matches no asset file"));
    }

    #[test]
    fn empty_field_fails() {
        let text = ONE.replace("SIL International", " ");
        let files = vec!["crates/fonts/assets/gentium/a.ttf".to_string()];
        let errors = check(&attribution(&text), &files).unwrap();
        assert!(errors[0].contains("empty `copyright`"));
    }
}
