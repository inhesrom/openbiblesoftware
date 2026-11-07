//! Parallel passages (#212, ADR 0014): the Passage Guide section listing the other accounts
//! of the Guide's Passage — the Gospels' tellings of one event, Chronicles beside Samuel and
//! Kings — from every Cross-reference set that carries Parallel passages, with one click to
//! Text Comparison at each (`compare.parallel`).
//!
//! A set lists, under each section heading of a Bible book, the Passages parallel to that
//! section (the BSB's `\r` lines). Its sections are often coarser on one side than the other:
//! Mark's "The Mission of John the Baptist" (Mark 1:1-11) lists Matthew 3:1-17, while
//! Matthew's own "The Baptism of Jesus" (Matthew 3:13-17) lists Mark 1:9-11. So each section is
//! read both ways, and the narrowest evidence wins:
//!
//! 1. A section overlapping the Passage offers its parallels; a parallel overlapping the
//!    Passage offers its section. What overlaps the Passage is that offer's *evidence*.
//! 2. An offer is dropped when another offer's evidence lies inside its own and covers at
//!    least as much of the Passage (Mark 1:1-11's offers give way to Mark 1:9-11's when the
//!    Passage is Mark 1:9-11).
//! 3. An offered Passage is dropped when another offered Passage lies inside it (Matthew 3:1-17
//!    gives way to Matthew 3:13-17), and when it overlaps the Guide's Passage itself.
//!
//! Rows are grouped by their evidence, in Bible order, each with the heading that paired them.

use obs_model::ParallelSection;
use obs_scripture::{Passage, PassageList, Standard};

/// Cross-reference sets that carry Parallel passages, by resource id: the Starter library's
/// BSB Parallel Passages. The section's empty state offers them.
pub const PARALLEL_SETS: &[&str] = &["bsb-parallels"];

/// Whether a Cross-reference set carries Parallel passages ([`PARALLEL_SETS`]).
pub fn is_parallel_set(resource_id: &str) -> bool {
    PARALLEL_SETS.contains(&resource_id)
}

/// One Parallel passage of the Guide's Passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParallelRow {
    /// The parallel Passage (Standard versification).
    pub passage: Passage,
    /// The heading of the section that pairs it with the Guide's Passage, e.g. `The Baptism
    /// of Jesus`.
    pub title: String,
    pub source_id: String,
    pub source_title: String,
}

/// The Parallel passages of one part of the Guide's Passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParallelGroup {
    /// The part of the Guide's Passage these rows are parallel to (it may reach past it: a
    /// Verse's whole section).
    pub passage: Passage,
    pub rows: Vec<ParallelRow>,
}

/// What the Parallel passages section shows for a Passage.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParallelPassages {
    pub groups: Vec<ParallelGroup>,
    /// Every parallel Passage, merged where they touch, in Bible order: `1 Chr 21` for 2
    /// Samuel 24's four sections.
    pub all: Vec<Passage>,
}

impl ParallelPassages {
    /// How many Parallel passages there are.
    pub fn count(&self) -> usize {
        self.groups.iter().map(|g| g.rows.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Every row, in order.
    pub fn rows(&self) -> impl Iterator<Item = &ParallelRow> {
        self.groups.iter().flat_map(|g| g.rows.iter())
    }
}

/// One offer of a parallel: the part that overlaps the Guide's Passage, and what it offers.
struct Offer<'a> {
    evidence: Passage,
    offered: Passage,
    title: &'a str,
    set: usize,
}

/// The Parallel passages of `passage` from `sets` (resource id, title, sections), in Preferred
/// order; a Passage two sets both offer is credited to the first.
pub fn parallels_for(
    passage: Passage,
    sets: &[(String, String, Vec<ParallelSection>)],
) -> ParallelPassages {
    let mut offers: Vec<Offer<'_>> = Vec::new();
    for (set, (_, _, sections)) in sets.iter().enumerate() {
        for s in sections {
            if s.passage.overlaps(passage) {
                offers.extend(s.parallels.iter().map(|p| Offer {
                    evidence: s.passage,
                    offered: *p,
                    title: &s.title,
                    set,
                }));
            }
            offers.extend(
                s.parallels
                    .iter()
                    .filter(|p| p.overlaps(passage))
                    .map(|p| Offer {
                        evidence: *p,
                        offered: s.passage,
                        title: &s.title,
                        set,
                    }),
            );
        }
    }
    offers.retain(|o| !o.offered.overlaps(passage));
    let covered = |e: Passage| e.intersection(passage);
    let dominated = |o: &Offer<'_>| {
        offers.iter().any(|other| {
            other.evidence != o.evidence
                && o.evidence.contains(other.evidence)
                && match (covered(other.evidence), covered(o.evidence)) {
                    (Some(narrow), Some(wide)) => narrow.contains(wide),
                    _ => false,
                }
        })
    };
    let keep: Vec<bool> = offers.iter().map(|o| !dominated(o)).collect();
    let mut offers: Vec<Offer<'_>> = offers
        .into_iter()
        .zip(keep)
        .filter_map(|(o, keep)| keep.then_some(o))
        .collect();
    let offered: Vec<Passage> = offers.iter().map(|o| o.offered).collect();
    offers.retain(|o| {
        !offered
            .iter()
            .any(|p| *p != o.offered && o.offered.contains(*p))
    });
    // One offer per offered Passage, credited to the first set (Preferred order) that offers
    // it, before ordering for display.
    offers.sort_by_key(|o| (o.set, o.evidence.start(), o.evidence.end()));
    let mut seen: Vec<Passage> = Vec::new();
    offers.retain(|o| {
        if seen.contains(&o.offered) {
            return false;
        }
        seen.push(o.offered);
        true
    });
    offers.sort_by_key(|o| (o.evidence.start(), o.evidence.end(), o.offered, o.set));
    let mut groups: Vec<ParallelGroup> = Vec::new();
    for o in &offers {
        let (id, title, _) = &sets[o.set];
        let row = ParallelRow {
            passage: o.offered,
            title: o.title.to_string(),
            source_id: id.clone(),
            source_title: title.clone(),
        };
        match groups.last_mut() {
            Some(g) if g.passage == o.evidence => g.rows.push(row),
            _ => groups.push(ParallelGroup {
                passage: o.evidence,
                rows: vec![row],
            }),
        }
    }
    let all = PassageList::from(seen)
        .merged(&Standard)
        .passages()
        .to_vec();
    ParallelPassages { groups, all }
}

/// Read every Cross-reference set's Parallel passages in Preferred order. A set that cannot
/// be read fails the section, as Cross-references do.
#[cfg(not(target_arch = "wasm32"))]
pub fn library_parallels(
    library: &crate::Library,
    passage: Passage,
) -> Result<ParallelPassages, String> {
    let mut sets = Vec::new();
    for entry in library.of_type(obs_model::ResourceType::CrossReferenceSet) {
        let reader = obs_store::ObrReader::open(&entry.path).map_err(|e| e.to_string())?;
        let sections = reader.parallel_sections().map_err(|e| e.to_string())?;
        if !sections.is_empty() {
            sets.push((
                entry.manifest.resource_id.clone(),
                entry.manifest.title.clone(),
                sections,
            ));
        }
    }
    Ok(parallels_for(passage, &sets))
}

#[cfg(target_arch = "wasm32")]
pub fn library_parallels(_: &crate::Library, _: Passage) -> Result<ParallelPassages, String> {
    Err("Opening Resources is not supported here".into())
}
