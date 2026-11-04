//! Golden-passage snapshots (#45, #20 decision 6.3).
//!
//! `starter/goldens/passages.toml` lists Keys to snapshot in named Resources; each snapshot is
//! committed as `starter/goldens/<resource_id>/<key slug>.json`. The build takes the same
//! snapshot of what it just built and fails on any difference, printing a unified diff; a
//! deliberate change is reviewed by committing the new file (`--update-goldens` rewrites
//! them), so the snapshot diff is part of the pull request.
//!
//! What a snapshot holds depends on the Resource type:
//! - Bible (`passage`, Standard versification): every Bible Verse that maps to it, with its own
//!   numbering, its Standard Passage, text, the spans over it (offsets relative to the Verse)
//!   and the note units those spans reference;
//! - Commentary (`passage`): the units whose Passage Key overlaps it, from the narrowest level
//!   that has any (verse sections, else the chapter introduction, else the book introduction);
//! - Lexicon (`strongs`): the entry units the number resolves to, with the extended → base
//!   fallback of `lookup_strongs`;
//! - Dictionary (`headword`): the entry units, aliases included;
//! - Cross-reference set (`passage`): the Cross-references from it;
//! - People & places index (`passage`): the mentions in it and the entities they name.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use obs_model::{
    CrossReference, Entity, Key, Mention, Resource, ResourceType, Span, SpanKind, TextUnit,
};
use obs_scripture::{Passage, Standard, StrongsNumber, parse_reference};
use serde::{Deserialize, Serialize};

pub const LIST_FILE: &str = "passages.toml";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenList {
    #[serde(rename = "golden", default)]
    pub goldens: Vec<Golden>,
}

/// One snapshot: a Resource and the Key to open it at (exactly one of the three).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Golden {
    pub resource_id: String,
    /// A Bible reference in the Standard versification.
    pub passage: Option<String>,
    pub strongs: Option<String>,
    pub headword: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GoldenKey {
    Passage(Passage),
    Strongs(StrongsNumber),
    Headword(String),
}

impl Golden {
    /// The Key as written in the list.
    pub fn key_text(&self) -> &str {
        self.passage
            .as_deref()
            .or(self.strongs.as_deref())
            .or(self.headword.as_deref())
            .unwrap_or("")
    }

    /// `<resource_id>/<key slug>.json`, relative to the goldens directory.
    pub fn file(&self) -> PathBuf {
        let mut slug = String::new();
        for c in self.key_text().chars().flat_map(char::to_lowercase) {
            if c.is_alphanumeric() {
                slug.push(c);
            } else if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        }
        let slug = slug.trim_end_matches('-');
        PathBuf::from(&self.resource_id).join(format!("{slug}.json"))
    }

    /// `kjv-strongs Gen 1:1`, for messages.
    pub fn name(&self) -> String {
        format!("{} {}", self.resource_id, self.key_text())
    }

    fn key(&self) -> Result<GoldenKey> {
        let count = [&self.passage, &self.strongs, &self.headword]
            .iter()
            .filter(|k| k.is_some())
            .count();
        if count != 1 {
            bail!(
                "golden `{}` must give exactly one of passage, strongs or headword",
                self.resource_id
            );
        }
        if let Some(text) = &self.passage {
            let list = parse_reference(text, &Standard, None)
                .map_err(|e| anyhow!("golden {}: {e}", self.name()))?;
            return match list.passages() {
                [one] => Ok(GoldenKey::Passage(*one)),
                _ => bail!("golden {}: one Passage expected", self.name()),
            };
        }
        if let Some(text) = &self.strongs {
            let n: StrongsNumber = text
                .parse()
                .map_err(|e| anyhow!("golden {}: {e}", self.name()))?;
            return Ok(GoldenKey::Strongs(n));
        }
        Ok(GoldenKey::Headword(
            self.headword.clone().unwrap_or_default(),
        ))
    }
}

impl GoldenList {
    /// Load `<dir>/passages.toml`; a directory without one lists no snapshots.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(LIST_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let list: Self =
            toml::from_str(&raw).with_context(|| format!("{} is not valid", path.display()))?;
        let mut files = BTreeSet::new();
        for g in &list.goldens {
            g.key()?;
            if !files.insert(g.file()) {
                bail!(
                    "golden {} shares its snapshot file {} with another entry",
                    g.name(),
                    g.file().display()
                );
            }
        }
        Ok(list)
    }
}

/// The snapshot of one Key in one Resource, as written to its `.json` file.
#[derive(Debug, Serialize)]
struct Snapshot<'a> {
    resource_id: &'a str,
    resource_type: ResourceType,
    key: &'a str,
    #[serde(flatten)]
    body: Body<'a>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Body<'a> {
    Verses {
        verses: Vec<VerseSnapshot<'a>>,
        notes: Vec<&'a TextUnit>,
    },
    Units {
        units: Vec<&'a TextUnit>,
    },
    CrossReferences {
        cross_references: Vec<&'a CrossReference>,
    },
    People {
        entities: Vec<&'a Entity>,
        mentions: Vec<&'a Mention>,
    },
}

/// One Bible Verse of a Bible snapshot. Span offsets are relative to the Verse's text.
#[derive(Debug, Serialize)]
struct VerseSnapshot<'a> {
    verse: String,
    last: u16,
    /// The Standard Passage, or `null` for an Unmapped verse.
    standard: Option<String>,
    unit: &'a str,
    text: &'a str,
    spans: Vec<Span>,
}

/// Snapshot `golden` from the built `resource`; an empty result is an error, because a Key
/// the golden expects has gone.
pub fn snapshot(golden: &Golden, resource: &Resource) -> Result<String> {
    let key = golden.key()?;
    let kind = resource.manifest.resource_type;
    let body = match (kind, &key) {
        (ResourceType::Bible, GoldenKey::Passage(p)) => bible(resource, *p),
        (ResourceType::Commentary, GoldenKey::Passage(p)) => Body::Units {
            units: commentary_sections(resource, *p),
        },
        (ResourceType::Lexicon, GoldenKey::Strongs(n)) => {
            let mut units = units_for(resource, |k| *k == Key::StrongsNumber(*n));
            if units.is_empty() && n.is_extended() {
                units = units_for(resource, |k| *k == Key::StrongsNumber(n.base()));
            }
            Body::Units { units }
        }
        (ResourceType::Dictionary, GoldenKey::Headword(h)) => Body::Units {
            units: units_for(resource, |k| matches!(k, Key::Headword(w) if w == h)),
        },
        (ResourceType::CrossReferenceSet, GoldenKey::Passage(p)) => Body::CrossReferences {
            cross_references: resource
                .cross_references
                .iter()
                .filter(|x| x.from.overlaps(*p))
                .collect(),
        },
        (ResourceType::PeopleAndPlaces, GoldenKey::Passage(p)) => {
            let mentions: Vec<&Mention> = resource
                .mentions
                .iter()
                .filter(|m| m.passage.overlaps(*p))
                .collect();
            let named: BTreeSet<&str> = mentions.iter().map(|m| m.entity.as_str()).collect();
            Body::People {
                entities: resource
                    .entities
                    .iter()
                    .filter(|e| named.contains(e.id.as_str()))
                    .collect(),
                mentions,
            }
        }
        _ => bail!(
            "golden {}: a {kind:?} is not opened by that kind of Key",
            golden.name()
        ),
    };
    let empty = match &body {
        Body::Verses { verses, .. } => verses.is_empty(),
        Body::Units { units } => units.is_empty(),
        Body::CrossReferences { cross_references } => cross_references.is_empty(),
        Body::People { mentions, .. } => mentions.is_empty(),
    };
    if empty {
        bail!(
            "golden {}: nothing in `{}` opens at that Key",
            golden.name(),
            golden.resource_id
        );
    }
    let snapshot = Snapshot {
        resource_id: &resource.manifest.resource_id,
        resource_type: kind,
        key: golden.key_text(),
        body,
    };
    let mut json = serde_json::to_string_pretty(&snapshot)?;
    json.push('\n');
    Ok(json)
}

/// The Commentary units whose Passage Key overlaps `passage`, from the narrowest level that
/// has any: verse sections first, else chapter introductions, else book introductions.
fn commentary_sections(resource: &Resource, passage: Passage) -> Vec<&TextUnit> {
    let level = |section: Passage| -> u8 {
        let start = section.start();
        if Passage::book(&Standard, start.book).is_some_and(|book| section.contains(book)) {
            2
        } else if Passage::chapter(&Standard, start.book, start.chapter)
            .is_some_and(|chapter| section.contains(chapter))
        {
            1
        } else {
            0
        }
    };
    let overlapping = |k: &Key| matches!(k, Key::Passage(section) if section.overlaps(passage));
    let Some(narrowest) = resource
        .keys
        .iter()
        .filter(|k| overlapping(&k.key))
        .filter_map(|k| match k.key {
            Key::Passage(section) => Some(level(section)),
            _ => None,
        })
        .min()
    else {
        return vec![];
    };
    units_for(resource, |k| match k {
        Key::Passage(section) => overlapping(k) && level(*section) == narrowest,
        _ => false,
    })
}

/// Units with a Key that `wanted` accepts, in document order.
fn units_for(resource: &Resource, wanted: impl Fn(&Key) -> bool) -> Vec<&TextUnit> {
    let ids: BTreeSet<_> = resource
        .keys
        .iter()
        .filter(|k| wanted(&k.key))
        .map(|k| &k.unit)
        .collect();
    resource
        .units
        .iter()
        .filter(|u| ids.contains(&u.id))
        .collect()
}

fn bible(resource: &Resource, passage: Passage) -> Body<'_> {
    let Some(map) = &resource.verse_map else {
        return Body::Verses {
            verses: vec![],
            notes: vec![],
        };
    };
    let mut native = Vec::new();
    for standard in passage.verses(&Standard) {
        for v in map.to_native(standard) {
            if !native.contains(&v) {
                native.push(v);
            }
        }
    }
    let mut verses = Vec::new();
    let mut notes = Vec::new();
    for v in native {
        let Some(entry) = resource.verses.iter().find(|e| e.verse == v) else {
            continue;
        };
        let Some(unit) = resource.units.iter().find(|u| u.id == entry.unit) else {
            continue;
        };
        let (start, end) = (entry.start, entry.end);
        let spans: Vec<Span> = unit
            .spans
            .iter()
            .filter(|s| {
                if s.start == s.end {
                    start <= s.start && s.start <= end
                } else {
                    s.start < end && s.end > start
                }
            })
            .map(|s| Span {
                start: s.start.max(start) - start,
                end: s.end.min(end) - start,
                kind: s.kind.clone(),
            })
            .collect();
        for s in &spans {
            if let SpanKind::NoteRef { note, .. } = &s.kind
                && let Some(n) = resource.units.iter().find(|u| &u.id == note)
                && !notes.iter().any(|seen: &&TextUnit| seen.id == n.id)
            {
                notes.push(n);
            }
        }
        verses.push(VerseSnapshot {
            verse: entry.verse.to_string(),
            last: entry.last,
            standard: map.to_standard(v).map(|p| p.to_string()),
            unit: unit.id.as_str(),
            text: &unit.text[entry.range()],
            spans,
        });
    }
    Body::Verses { verses, notes }
}

/// How one snapshot compares with its committed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Unchanged,
    /// The unified diff, committed → built.
    Changed(String),
    /// No committed file yet.
    New,
    /// Rewritten by `--update-goldens` (changed or new).
    Written,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub name: String,
    /// Snapshot file, relative to the goldens directory.
    pub file: PathBuf,
    pub status: Status,
}

impl Checked {
    pub fn passed(&self) -> bool {
        matches!(self.status, Status::Unchanged | Status::Written)
    }
}

/// Snapshot every golden from the built Resources and compare it with the committed file in
/// `dir` (or rewrite the file when `update`). Fails when a golden names a Resource that is not
/// in the build or a Key that opens nothing.
pub fn check(
    list: &GoldenList,
    resources: &[&Resource],
    dir: &Path,
    update: bool,
) -> Result<Vec<Checked>> {
    let mut checked = Vec::with_capacity(list.goldens.len());
    for g in &list.goldens {
        let resource = resources
            .iter()
            .find(|r| r.manifest.resource_id == g.resource_id)
            .ok_or_else(|| {
                anyhow!(
                    "golden {}: `{}` is not in the lock",
                    g.name(),
                    g.resource_id
                )
            })?;
        let actual = snapshot(g, resource)?;
        let file = g.file();
        let path = dir.join(&file);
        let committed = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };
        let status = match committed {
            Some(text) if text == actual => Status::Unchanged,
            _ if update => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, &actual)
                    .with_context(|| format!("cannot write {}", path.display()))?;
                Status::Written
            }
            Some(text) => Status::Changed(unified_diff(&text, &actual)),
            None => Status::New,
        };
        checked.push(Checked {
            name: g.name(),
            file,
            status,
        });
    }
    Ok(checked)
}

/// A unified diff of two texts, by line, with three lines of context.
pub fn unified_diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    #[derive(Clone, Copy, PartialEq)]
    enum Op {
        Keep,
        Del,
        Add,
    }
    // Lines that differ are usually few and local: match the common head and tail first, then
    // align the middle by longest common subsequence (or, past a size cap, replace it whole).
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (mid_a, mid_b) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    let (n, m) = (mid_a.len(), mid_b.len());
    let mut ops: Vec<(Op, usize, usize)> = (0..head).map(|i| (Op::Keep, i, i)).collect();
    const MAX_CELLS: usize = 16_000_000;
    if n.saturating_mul(m) > MAX_CELLS {
        ops.extend((0..n).map(|i| (Op::Del, head + i, head)));
        ops.extend((0..m).map(|j| (Op::Add, head + n, head + j)));
    } else {
        let mut lcs = vec![vec![0u32; m + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i][j] = if mid_a[i] == mid_b[j] {
                    lcs[i + 1][j + 1] + 1
                } else {
                    lcs[i + 1][j].max(lcs[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && mid_a[i] == mid_b[j] {
                ops.push((Op::Keep, head + i, head + j));
                i += 1;
                j += 1;
            } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
                ops.push((Op::Del, head + i, head + j));
                i += 1;
            } else {
                ops.push((Op::Add, head + i, head + j));
                j += 1;
            }
        }
    }
    ops.extend((0..tail).map(|k| (Op::Keep, head + n + k, head + m + k)));
    const CONTEXT: usize = 3;
    let mut out = String::from("--- committed\n+++ built\n");
    let mut at = 0;
    while at < ops.len() {
        let Some(first) = ops[at..].iter().position(|(op, ..)| *op != Op::Keep) else {
            break;
        };
        let start = (at + first).saturating_sub(CONTEXT);
        let mut end = at + first;
        // Extend the hunk while the next change is within 2*CONTEXT kept lines.
        loop {
            let next = ops[end..].iter().position(|(op, ..)| *op != Op::Keep);
            match next {
                Some(k) if k <= 2 * CONTEXT => {
                    end += k + 1;
                }
                _ => break,
            }
        }
        let end = (end + CONTEXT).min(ops.len());
        let (old_start, new_start) = (ops[start].1, ops[start].2);
        let old_len = ops[start..end]
            .iter()
            .filter(|(op, ..)| *op != Op::Add)
            .count();
        let new_len = ops[start..end]
            .iter()
            .filter(|(op, ..)| *op != Op::Del)
            .count();
        let _ = writeln!(
            out,
            "@@ -{},{old_len} +{},{new_len} @@",
            old_start + 1,
            new_start + 1
        );
        for (op, i, j) in &ops[start..end] {
            match op {
                Op::Keep => {
                    let _ = writeln!(out, " {}", a[*i]);
                }
                Op::Del => {
                    let _ = writeln!(out, "-{}", a[*i]);
                }
                Op::Add => {
                    let _ = writeln!(out, "+{}", b[*j]);
                }
            }
        }
        at = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden(resource_id: &str, passage: &str) -> Golden {
        Golden {
            resource_id: resource_id.into(),
            passage: Some(passage.into()),
            strongs: None,
            headword: None,
        }
    }

    #[test]
    fn snapshot_files_are_slugged_from_the_key() {
        assert_eq!(
            golden("kjv-strongs", "Ps 3:title").file(),
            PathBuf::from("kjv-strongs/ps-3-title.json")
        );
        assert_eq!(
            golden("web", "3John 1:14").file(),
            PathBuf::from("web/3john-1-14.json")
        );
    }

    #[test]
    fn a_golden_needs_exactly_one_key() {
        let mut g = golden("web", "Gen 1:1");
        g.strongs = Some("G5485".into());
        let err = g.key().unwrap_err().to_string();
        assert!(
            err.contains("exactly one of passage, strongs or headword"),
            "{err}"
        );
    }

    #[test]
    fn a_bad_reference_names_the_golden() {
        let err = golden("web", "Nothing 99:1").key().unwrap_err().to_string();
        assert!(err.contains("web Nothing 99:1"), "{err}");
    }

    #[test]
    fn the_list_refuses_entries_sharing_a_snapshot_file() {
        let dir = std::env::temp_dir().join(format!(
            "obs-goldens-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(LIST_FILE),
            "[[golden]]\nresource_id = \"web\"\npassage = \"Gen 1:1\"\n\n\
             [[golden]]\nresource_id = \"web\"\npassage = \"Gen 1.1\"\n",
        )
        .unwrap();
        let err = GoldenList::load(&dir).unwrap_err().to_string();
        assert!(err.contains("shares its snapshot file"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_diff_is_unified_with_hunk_headers_and_context() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
        let new = "a\nb\nc\nd\nE\nf\ng\nh\ni\nj\n";
        let diff = unified_diff(old, new);
        assert!(diff.starts_with("--- committed\n+++ built\n"), "{diff}");
        assert!(diff.contains("@@ -2,7 +2,7 @@"), "{diff}");
        assert!(diff.contains("-e\n+E\n"), "{diff}");
        assert!(
            !diff.contains("\n a\n"),
            "lines outside the context stay out: {diff}"
        );
    }

    #[test]
    fn identical_texts_diff_to_nothing_but_the_header() {
        assert_eq!(
            unified_diff("same\n", "same\n"),
            "--- committed\n+++ built\n"
        );
    }
}
