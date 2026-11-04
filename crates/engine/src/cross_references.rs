//! Shared data for the Cross-references Tool and Passage Guide. Call file-backed queries on
//! the job queue, never on the UI thread. Every query speaks Standard versification.

use std::collections::BTreeMap;

use obs_model::{CrossReference, Direction};
use obs_scripture::{Passage, Standard, Verse};
use obs_store::{ResourceStore, StoreError};

pub const TOP_REFERENCES: usize = 8;
pub const PREVIEW_VERSES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceRow {
    pub source_id: String,
    pub source_title: String,
    pub target: Passage,
    pub votes: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceGroup {
    pub verse: Verse,
    pub rows: Vec<ReferenceRow>,
}

/// Combine sets in Preferred order. Votes rank each source Verse's targets; ties retain
/// Preferred order, then target order. Duplicate targets from different sets keep attribution.
pub fn group_references(
    passage: Passage,
    sets: impl IntoIterator<Item = (String, String, Vec<CrossReference>)>,
) -> Vec<ReferenceGroup> {
    let mut groups: BTreeMap<Verse, Vec<ReferenceRow>> = BTreeMap::new();
    for (id, title, mut rows) in sets {
        rows.sort_by_key(|r| r.to);
        for row in rows {
            for verse in row
                .from
                .verses(&Standard)
                .filter(|v| passage.contains_verse(*v))
            {
                groups.entry(verse).or_default().push(ReferenceRow {
                    source_id: id.clone(),
                    source_title: title.clone(),
                    target: row.to,
                    votes: row.votes,
                });
            }
        }
    }
    groups
        .into_iter()
        .map(|(verse, mut rows)| {
            rows.sort_by_key(|row| std::cmp::Reverse(row.votes));
            ReferenceGroup { verse, rows }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiblePreview {
    pub abbreviation: String,
    pub direction: Direction,
    pub text: String,
    pub truncated: bool,
}

/// Exact mapped text, not the nearest preceding Verse used for navigation. An Absent verse
/// must never silently preview another Verse. Keep work bounded for chapter/book references.
pub fn bible_preview(
    store: &dyn ResourceStore,
    passage: Passage,
) -> Result<BiblePreview, StoreError> {
    BiblePreviewCache::default().preview(store, passage)
}

/// Worker-owned cache: load the Verse map once per Resource revision and retain a few Bible
/// books. Hovering another target must not re-read and re-index an entire Bible's Verse map.
#[derive(Default)]
pub struct BiblePreviewCache {
    identity: Option<(String, u32, String)>,
    map: Option<obs_scripture::VerseMap>,
    units: BTreeMap<obs_model::UnitId, Option<obs_model::TextUnit>>,
}

impl BiblePreviewCache {
    pub fn preview(
        &mut self,
        store: &dyn ResourceStore,
        passage: Passage,
    ) -> Result<BiblePreview, StoreError> {
        let m = store.manifest();
        let identity = (m.resource_id.clone(), m.revision, m.content_hash.clone());
        if self.identity.as_ref() != Some(&identity) {
            self.map = store.verse_map()?;
            self.units.clear();
            self.identity = Some(identity);
        }
        if self.units.len() > 4 {
            self.units.clear();
        }
        let map = &self.map;
        let units = &mut self.units;
        let mut text = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut verses = passage.verses(&Standard);
        for standard in verses.by_ref().take(PREVIEW_VERSES) {
            let native = match map {
                Some(m) => m.to_native(standard),
                None => vec![standard],
            };
            let mut found = false;
            for native in native {
                let Some(v) = store.verse(native)? else {
                    continue;
                };
                found = true;
                if !seen.insert((v.unit.clone(), v.start, v.end)) {
                    continue;
                }
                if !units.contains_key(&v.unit) {
                    units.insert(v.unit.clone(), store.unit(&v.unit)?);
                }
                if let Some(Some(unit)) = units.get(&v.unit)
                    && let Some(slice) = unit.text.get(v.start as usize..v.end as usize)
                {
                    text.push(slice.trim().to_string());
                }
            }
            if !found {
                text.push(format!("{standard}: — not in this Bible —"));
            }
        }
        Ok(BiblePreview {
            abbreviation: store.manifest().abbreviation.clone(),
            direction: store.manifest().direction,
            text: text.join(" "),
            truncated: verses.next().is_some(),
        })
    }
}

/// Read every Cross-reference set in Preferred order. Errors remain visible to the caller.
#[cfg(not(target_arch = "wasm32"))]
pub fn library_references(
    library: &crate::Library,
    passage: Passage,
) -> Result<Vec<ReferenceGroup>, String> {
    let mut sets = Vec::new();
    for entry in library.of_type(obs_model::ResourceType::CrossReferenceSet) {
        let reader = obs_store::ObrReader::open(&entry.path).map_err(|e| e.to_string())?;
        sets.push((
            entry.manifest.resource_id.clone(),
            entry.manifest.title.clone(),
            reader
                .cross_references_from(passage)
                .map_err(|e| e.to_string())?,
        ));
    }
    Ok(group_references(passage, sets))
}

/// Preview from the ★ Bible, never silently falling back to a lower-ranked Bible.
#[cfg(not(target_arch = "wasm32"))]
pub fn preferred_preview(
    library: &crate::Library,
    passage: Passage,
) -> Result<BiblePreview, String> {
    preferred_preview_cached(library, passage, &mut BiblePreviewCache::default())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn preferred_preview_cached(
    library: &crate::Library,
    passage: Passage,
    cache: &mut BiblePreviewCache,
) -> Result<BiblePreview, String> {
    let entry = library
        .of_type(obs_model::ResourceType::Bible)
        .next()
        .ok_or("Add a Bible to your library to see previews")?;
    let reader = obs_store::ObrReader::open(&entry.path).map_err(|e| e.to_string())?;
    cache.preview(&reader, passage).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn library_references(_: &crate::Library, _: Passage) -> Result<Vec<ReferenceGroup>, String> {
    Err("Opening Resources is not supported here".into())
}

#[cfg(target_arch = "wasm32")]
pub fn preferred_preview(_: &crate::Library, _: Passage) -> Result<BiblePreview, String> {
    Err("Opening Resources is not supported here".into())
}

#[cfg(target_arch = "wasm32")]
pub fn preferred_preview_cached(
    library: &crate::Library,
    passage: Passage,
    _: &mut BiblePreviewCache,
) -> Result<BiblePreview, String> {
    preferred_preview(library, passage)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use obs_scripture::Book;

    struct Files(std::path::PathBuf);
    impl Files {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("obs-references-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn add(&self, library: &mut crate::Library, resource: &obs_model::Resource) {
            let path = self
                .0
                .join(format!("{}.obr", resource.manifest.resource_id));
            obs_store::write_obr(resource, &path).unwrap();
            library.add_file(&path).unwrap();
        }
    }
    impl Drop for Files {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn john_3_16_openbible_targets_are_in_vote_order() {
        let files = Files::new("openbible");
        let mut library = crate::Library::new();
        let options = obs_formats::DatasetOptions {
            resource_id: "openbible".into(),
            title: "OpenBible.info".into(),
            ..Default::default()
        };
        let (resource, _) = obs_formats::import_openbible_cross_references(
            include_str!("../../formats/tests/assets/openbible/cross-references-excerpt.txt"),
            &options,
        )
        .unwrap();
        files.add(&mut library, &resource);
        let groups =
            library_references(&library, Passage::verse(Verse::new(Book::John, 3, 16))).unwrap();
        let snapshot = groups[0]
            .rows
            .iter()
            .map(|r| format!("{}: {}", r.target, r.votes))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            snapshot,
            "John 11:25-26: 465\nJohn 6:40: 404\n1 Tim 1:15-16: 291\nJohn 1:29: 240\nEph 2:4: 206\nJohn 1:18: 152"
        );
    }

    #[test]
    fn preferred_preview_maps_exactly_and_does_not_substitute_absent_verses() {
        let files = Files::new("preview");
        let mut library = crate::Library::new();
        let resource = obs_store::sample_resource();
        files.add(&mut library, &resource);
        let john = |v| Passage::verse(Verse::new(Book::John, 3, v));
        assert!(
            preferred_preview(&library, john(16))
                .unwrap()
                .text
                .starts_with("For God so loved")
        );
        assert!(
            preferred_preview(&library, john(18))
                .unwrap()
                .text
                .contains("not in this Bible")
        );
        let mut mapped = resource.clone();
        mapped.manifest.resource_id = "mapped".into();
        mapped.manifest.abbreviation = "MAPPED".into();
        mapped.verse_map = Some(obs_scripture::VerseMap::new(
            mapped
                .verses
                .iter()
                .map(|v| obs_scripture::VerseMapEntry {
                    verse: v.verse,
                    last: v.last,
                    standard: (v.verse == john(16).start()).then_some(john(18)),
                })
                .collect(),
            vec![],
            &Standard,
        ));
        // Preview the in-memory numbering through a real .obr; refresh the content hash.
        mapped.manifest.content_hash = mapped.content_hash();
        let mut reordered = crate::Library::new();
        files.add(&mut reordered, &mapped);
        files.add(&mut reordered, &resource);
        let preview = preferred_preview(&reordered, john(18)).unwrap();
        assert_eq!(preview.abbreviation, "MAPPED");
        assert!(preview.text.starts_with("For God so loved"));
        assert!(
            preferred_preview(&reordered, john(17))
                .unwrap()
                .text
                .contains("not in this Bible")
        );
        let chapter = Passage::chapter(&Standard, Book::John, 3).unwrap();
        assert!(preferred_preview(&library, chapter).unwrap().truncated);
        assert!(preferred_preview(&crate::Library::new(), john(16)).is_err());
    }

    #[test]
    fn groups_each_source_verse_and_ranks_votes_with_preferred_order_ties() {
        let a = Verse::new(Book::John, 3, 16);
        let b = Verse::new(Book::John, 3, 17);
        let passage = Passage::new(a, b).unwrap();
        let row = |from, to, votes| CrossReference {
            from,
            to: Passage::verse(to),
            votes,
        };
        let groups = group_references(
            passage,
            [
                (
                    "first".into(),
                    "First".into(),
                    vec![row(passage, a, -1), row(Passage::verse(a), b, 9)],
                ),
                (
                    "second".into(),
                    "Second".into(),
                    vec![row(Passage::verse(a), a, 9)],
                ),
            ],
        );
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups[0]
                .rows
                .iter()
                .map(|r| (r.source_id.as_str(), r.votes))
                .collect::<Vec<_>>(),
            [("first", 9), ("second", 9), ("first", -1)]
        );
        assert_eq!(groups[1].verse, b);
        assert_eq!(groups[1].rows.len(), 1);
        assert!(group_references(Passage::verse(a), []).is_empty());
    }
}
