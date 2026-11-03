//! `starter/sources.toml` (the lock) and `starter/id-changes.toml`, and the provenance gate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use obs_model::is_valid_resource_id;
use serde::Deserialize;

/// The parsed lock file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    /// Starter set plus Search index caches must fit in this many megabytes (10^6 bytes).
    pub size_budget_mb: u64,
    #[serde(rename = "source", default)]
    pub sources: Vec<Source>,
}

/// One locked source: where it comes from, what it becomes, and why we may ship it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Stable slug of the Resource this source becomes. Never reused.
    pub resource_id: String,
    /// Which Import parser reads it: `usfm` (a zip of USFM books).
    pub importer: String,
    /// The pinned download. `file://` paths are allowed (tests, local mirrors).
    pub url: String,
    /// Where the publisher serves the file, when `url` is a mirror or an archived copy.
    pub origin_url: Option<String>,
    /// Lowercase hex sha256 of the file at `url`.
    pub sha256: String,
    pub title: String,
    pub abbreviation: String,
    /// BCP 47 language tag.
    pub language: String,
    pub edition: Option<String>,
    pub publisher: Option<String>,
    /// Shown as the source URL in About.
    pub homepage: Option<String>,
    /// SPDX licence expression (`LicenseRef-PublicDomain` for public-domain texts).
    pub spdx: String,
    /// Credit text shown verbatim in About.
    pub attribution: String,
    /// "Changes we made", shown in About.
    pub changes: String,
    /// Where the licence claim comes from, in one or two sentences.
    pub provenance: String,
    /// Evidence file, relative to the lock's directory. It must name `sha256`.
    pub evidence: String,
    /// Source book codes (`\id`) to leave out, e.g. eBible front matter.
    #[serde(default)]
    pub skip_books: Vec<String>,
}

/// The parsed `starter/id-changes.toml`: cumulative, never pruned.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdChanges {
    /// `resource_id`s that were shipped once and are gone for good. They may not come back.
    #[serde(default)]
    pub retired: Vec<String>,
    /// Text-unit ids that were removed or renamed; shipped as the `.obr` `unit_alias` table.
    #[serde(rename = "alias", default)]
    pub aliases: Vec<IdChange>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdChange {
    pub resource_id: String,
    pub old: String,
    pub new: String,
}

pub const IMPORTERS: &[&str] = &["usfm"];

/// Hosts whose site terms restrict republishing (#20 decision 3).
const REFUSED_HOSTS: &[&str] = &["ccel.org"];

impl Lock {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("{} is not a valid lock", path.display()))
    }
}

impl IdChanges {
    /// A missing file means no id changes yet.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("{} is not valid", path.display()))
    }

    pub fn for_resource<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a IdChange> + 'a {
        self.aliases.iter().filter(move |c| c.resource_id == id)
    }
}

/// Whether `url` points at a host we refuse, directly or through an archive wrapper.
pub fn refused_host(url: &str) -> Option<&'static str> {
    // Archive and redirect wrappers may percent-encode the wrapped URL.
    let lower = url
        .to_ascii_lowercase()
        .replace("%2f", "/")
        .replace("%2e", ".")
        .replace("%40", "@")
        .replace("%3a", ":");
    REFUSED_HOSTS.iter().copied().find(|host| {
        lower
            .match_indices(host)
            .any(|(i, _)| i == 0 || matches!(lower.as_bytes()[i - 1], b'/' | b'.' | b'@'))
    })
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The provenance gate and the other lock rules. Returns every problem, not just the first.
pub fn check(lock: &Lock, lock_dir: &Path, changes: &IdChanges) -> Result<()> {
    let mut problems: Vec<String> = Vec::new();
    let mut seen = BTreeSet::new();
    let retired: BTreeSet<&str> = changes.retired.iter().map(String::as_str).collect();
    for s in &lock.sources {
        let id = s.resource_id.as_str();
        let mut bad = |why: String| problems.push(format!("{id}: {why}"));
        if !is_valid_resource_id(id) {
            bad("resource_id must be 1-64 chars of a-z, 0-9 and `-`".into());
        }
        if !seen.insert(id) {
            bad("resource_id is listed twice".into());
        }
        if retired.contains(id) {
            bad("resource_id was retired in id-changes.toml and may not be reused".into());
        }
        if !IMPORTERS.contains(&s.importer.as_str()) {
            bad(format!(
                "unknown importer `{}` (known: {})",
                s.importer,
                IMPORTERS.join(", ")
            ));
        }
        if !is_sha256(&s.sha256) {
            bad("sha256 must be 64 lowercase hex digits".into());
        }
        if !(s.url.starts_with("https://")
            || s.url.starts_with("http://")
            || s.url.starts_with("file://"))
        {
            bad(format!("url `{}` must be http(s):// or file://", s.url));
        }
        for url in std::iter::once(&s.url)
            .chain(&s.origin_url)
            .chain(&s.homepage)
        {
            if let Some(host) = refused_host(url) {
                bad(format!(
                    "`{url}` is hosted on {host}, whose terms restrict republishing; \
                     use another host"
                ));
            }
        }
        for (name, value) in [
            ("title", &s.title),
            ("abbreviation", &s.abbreviation),
            ("language", &s.language),
            ("spdx", &s.spdx),
            ("attribution", &s.attribution),
            ("changes", &s.changes),
            ("provenance", &s.provenance),
        ] {
            if value.trim().is_empty() {
                bad(format!("`{name}` is empty"));
            }
        }
        if let Some(why) = evidence_problem(lock_dir, s) {
            bad(why);
        }
    }
    let mut olds = BTreeSet::new();
    for c in &changes.aliases {
        let at = format!("id-changes.toml alias {} `{}`", c.resource_id, c.old);
        if !seen.contains(c.resource_id.as_str()) && !retired.contains(c.resource_id.as_str()) {
            problems.push(format!(
                "{at}: no lock entry has resource_id `{}`",
                c.resource_id
            ));
        }
        if c.old == c.new {
            problems.push(format!("{at}: maps an id to itself"));
        }
        if !olds.insert((c.resource_id.as_str(), c.old.as_str())) {
            problems.push(format!("{at}: the old id is mapped twice"));
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    bail!(
        "the lock failed the provenance gate:\n  {}",
        problems.join("\n  ")
    )
}

/// Why `source`'s evidence is not acceptable, if it isn't.
fn evidence_problem(lock_dir: &Path, s: &Source) -> Option<String> {
    if s.evidence.trim().is_empty() {
        return Some("no evidence file; every entry must cite archived licence evidence".into());
    }
    let rel = Path::new(&s.evidence);
    if rel.is_absolute() || rel.components().any(|c| c.as_os_str() == "..") {
        return Some(format!(
            "evidence `{}` must be a path inside the lock's directory",
            s.evidence
        ));
    }
    let path: PathBuf = lock_dir.join(rel);
    match std::fs::read_to_string(&path) {
        Err(_) => Some(format!("evidence file `{}` is missing", s.evidence)),
        // Tie the evidence to the exact edition shipped.
        Ok(text) if !text.contains(&s.sha256) => Some(format!(
            "evidence `{}` does not name the pinned sha256 {}",
            s.evidence, s.sha256
        )),
        Ok(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ccel_is_refused_even_through_an_archive() {
        assert_eq!(refused_host("https://www.ccel.org/x.zip"), Some("ccel.org"));
        assert_eq!(refused_host("https://CCEL.org/x"), Some("ccel.org"));
        assert_eq!(
            refused_host("https://web.archive.org/web/2020id_/https://ccel.org/h/henry.zip"),
            Some("ccel.org")
        );
        assert_eq!(
            refused_host("https://example.org/fetch?u=https%3A%2F%2Fwww.ccel.org%2Fx"),
            Some("ccel.org")
        );
        assert_eq!(refused_host("https://ebible.org/Scriptures/x.zip"), None);
        assert_eq!(refused_host("https://notccel.org/x"), None);
    }

    #[test]
    fn id_changes_entries_are_checked() {
        let lock = Lock {
            size_budget_mb: 1,
            sources: Vec::new(),
        };
        let change = |r: &str, o: &str, n: &str| IdChange {
            resource_id: r.into(),
            old: o.into(),
            new: n.into(),
        };
        let changes = IdChanges {
            retired: Vec::new(),
            aliases: vec![
                change("nope", "A", "B"),
                change("nope", "A", "C"),
                change("nope", "D", "D"),
            ],
        };
        let err = check(&lock, Path::new("."), &changes)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no lock entry"), "{err}");
        assert!(err.contains("mapped twice"), "{err}");
        assert!(err.contains("maps an id to itself"), "{err}");
    }

    #[test]
    fn sha256_shape() {
        assert!(is_sha256(&"a".repeat(64)));
        assert!(!is_sha256(&"A".repeat(64)));
        assert!(!is_sha256("abc"));
    }
}
