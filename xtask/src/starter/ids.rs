//! The release manifest and the stable-id diff against the previous release (#20 decision 2).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::lock::IdChanges;

pub const MANIFEST_FORMAT: u32 = 1;

/// What a release shipped: per Resource, its revision, hashes and every unit id it answers to.
/// Published with each release; the next build diffs against it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub format: u32,
    pub resources: Vec<ReleaseResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseResource {
    pub resource_id: String,
    pub revision: u32,
    /// The Resource's content hash (`sha256:…`).
    pub content_hash: String,
    /// sha256 of the `.obr` file.
    pub file_sha256: String,
    pub bytes: u64,
    /// Live text-unit ids, in document order.
    pub units: Vec<String>,
    /// Retired unit id → its replacement (the `unit_alias` table).
    pub unit_aliases: BTreeMap<String, String>,
}

impl ReleaseManifest {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let m: Self = serde_json::from_str(&raw)
            .with_context(|| format!("{} is not a release manifest", path.display()))?;
        if m.format != MANIFEST_FORMAT {
            bail!(
                "{} has manifest format {}, expected {MANIFEST_FORMAT}",
                path.display(),
                m.format
            );
        }
        Ok(m)
    }

    pub fn get(&self, id: &str) -> Option<&ReleaseResource> {
        self.resources.iter().find(|r| r.resource_id == id)
    }
}

/// How one Resource changed since the previous release.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IdDiff {
    /// No previous release carried this Resource.
    pub new_resource: bool,
    pub added: Vec<String>,
    /// Ids that left the unit list and are now reached through an alias.
    pub aliased: Vec<String>,
    pub content_changed: bool,
    /// Revision for this build: kept when the content is unchanged, else bumped.
    pub revision: u32,
}

/// The unit aliases a Resource ships: every `id-changes.toml` entry for it.
pub fn aliases_for(id: &str, changes: &IdChanges) -> BTreeMap<String, String> {
    changes
        .for_resource(id)
        .map(|c| (c.old.clone(), c.new.clone()))
        .collect()
}

/// Diff one built Resource against its previous release. Fails when a previously reachable
/// unit id (live or aliased) is no longer reachable.
pub fn diff(
    previous: Option<&ReleaseResource>,
    id: &str,
    units: &[String],
    aliases: &BTreeMap<String, String>,
    content_hash: &str,
) -> Result<IdDiff> {
    let Some(prev) = previous else {
        return Ok(IdDiff {
            new_resource: true,
            added: units.to_vec(),
            revision: 1,
            ..IdDiff::default()
        });
    };
    let live: BTreeSet<&str> = units.iter().map(String::as_str).collect();
    let reachable: BTreeSet<&str> = live
        .iter()
        .copied()
        .chain(aliases.keys().map(String::as_str))
        .collect();
    let prev_live: BTreeSet<&str> = prev.units.iter().map(String::as_str).collect();
    let missing: Vec<&str> = prev_live
        .iter()
        .copied()
        .chain(prev.unit_aliases.keys().map(String::as_str))
        .filter(|u| !reachable.contains(u))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !missing.is_empty() {
        let shown: Vec<&str> = missing.iter().copied().take(20).collect();
        bail!(
            "{id}: {} text-unit id(s) from the previous release are gone: {}{}\n  \
             Annotations anchored to them would be orphaned. Map each old id to its replacement \
             in starter/id-changes.toml:\n  [[alias]]\n  resource_id = \"{id}\"\n  old = \"{}\"\n  new = \"…\"",
            missing.len(),
            shown.join(", "),
            if missing.len() > shown.len() {
                ", …"
            } else {
                ""
            },
            missing[0]
        );
    }
    let content_changed = prev.content_hash != content_hash;
    Ok(IdDiff {
        new_resource: false,
        added: units
            .iter()
            .filter(|u| !prev_live.contains(u.as_str()))
            .cloned()
            .collect(),
        aliased: prev_live
            .iter()
            .filter(|u| !live.contains(*u))
            .map(|u| u.to_string())
            .collect(),
        content_changed,
        revision: if content_changed {
            prev.revision + 1
        } else {
            prev.revision
        },
    })
}

/// Previously shipped `resource_id`s that this build drops must be retired explicitly.
pub fn check_dropped_resources(
    previous: &ReleaseManifest,
    built: &BTreeSet<&str>,
    changes: &IdChanges,
) -> Result<()> {
    let dropped: Vec<&str> = previous
        .resources
        .iter()
        .map(|r| r.resource_id.as_str())
        .filter(|id| !built.contains(id) && !changes.retired.iter().any(|r| r == id))
        .collect();
    if dropped.is_empty() {
        return Ok(());
    }
    bail!(
        "Resource(s) shipped in the previous release are missing from the lock: {}\n  \
         Users' Annotations point at them. Add them to `retired` in starter/id-changes.toml \
         only if they are gone for good.",
        dropped.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prev(units: &[&str], aliases: &[(&str, &str)]) -> ReleaseResource {
        ReleaseResource {
            resource_id: "bsb".into(),
            revision: 3,
            content_hash: "sha256:old".into(),
            file_sha256: String::new(),
            bytes: 0,
            units: units.iter().map(|s| s.to_string()).collect(),
            unit_aliases: aliases
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn first_release_is_revision_one() {
        let d = diff(None, "bsb", &ids(&["GEN"]), &BTreeMap::new(), "h").unwrap();
        assert!(d.new_resource);
        assert_eq!(d.revision, 1);
    }

    #[test]
    fn unchanged_content_keeps_the_revision_and_changed_content_bumps_it() {
        let p = prev(&["GEN", "EXO"], &[]);
        let units = ids(&["GEN", "EXO", "LEV"]);
        let same = diff(Some(&p), "bsb", &units, &BTreeMap::new(), "sha256:old").unwrap();
        assert_eq!((same.revision, same.content_changed), (3, false));
        assert_eq!(same.added, ids(&["LEV"]));
        let changed = diff(Some(&p), "bsb", &units, &BTreeMap::new(), "sha256:new").unwrap();
        assert_eq!((changed.revision, changed.content_changed), (4, true));
    }

    #[test]
    fn a_removed_unit_fails_unless_aliased() {
        let p = prev(&["GEN", "EXO"], &[]);
        let units = ids(&["GEN"]);
        let err = diff(Some(&p), "bsb", &units, &BTreeMap::new(), "h").unwrap_err();
        assert!(err.to_string().contains("EXO"), "{err}");
        let aliases = BTreeMap::from([("EXO".to_string(), "GEN".to_string())]);
        let d = diff(Some(&p), "bsb", &units, &aliases, "h").unwrap();
        assert_eq!(d.aliased, ids(&["EXO"]));
    }

    #[test]
    fn a_dropped_alias_fails_too() {
        let p = prev(&["GEN"], &[("OLD", "GEN")]);
        let err = diff(Some(&p), "bsb", &ids(&["GEN"]), &BTreeMap::new(), "h").unwrap_err();
        assert!(err.to_string().contains("OLD"), "{err}");
    }

    #[test]
    fn dropped_resources_must_be_retired() {
        let m = ReleaseManifest {
            format: MANIFEST_FORMAT,
            resources: vec![prev(&["GEN"], &[])],
        };
        let built = BTreeSet::new();
        assert!(check_dropped_resources(&m, &built, &IdChanges::default()).is_err());
        let retired = IdChanges {
            retired: vec!["bsb".into()],
            ..IdChanges::default()
        };
        assert!(check_dropped_resources(&m, &built, &retired).is_ok());
    }
}
