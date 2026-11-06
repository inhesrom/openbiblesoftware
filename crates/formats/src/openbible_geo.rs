//! OpenBible.info Bible Geocoding Data (`ancient.jsonl` and `modern.jsonl`, CC BY 4.0) → a
//! People & places index whose places carry Place locations (#150).
//!
//! Both files are JSON Lines. `ancient.jsonl` has one record per place as the Bible names it:
//! its `verses` (OSIS, ESV numbering, with `alternate_verses.kjv` where the KJV differs), its
//! `modern_associations` (each modern location it has been identified with, with a score where
//! 500 and above is high confidence) and its `linked_data` (among others, the TIPNR unique name
//! it was matched to when the data was made). `modern.jsonl` has one record per modern location:
//! its `lonlat` and where the coordinates come from (`coordinates_source.type`).
//!
//! Kept:
//!
//! | source | kept as |
//! |---|---|
//! | the place's verses (the KJV's number where it differs) | mentions, one per Verse |
//! | each modern association with coordinates and a score above 0 | a [`PlaceLocation`]: modern name, coordinates, score (capped at 1000) |
//! | the spellings English Bibles use (`translation_name_counts`) | `other_names` |
//!
//! **Matched to TIPNR.** Given the People & places index of STEPBible's TIPNR, each place takes
//! the id and name of the TIPNR place it is: the one with the place's name (its TIPNR link's
//! name, its own name without the number OpenBible adds, or a spelling Bibles use) that shares
//! the most Verses with it. TIPNR treats some places OpenBible lists twice as one (Acco and
//! Ptolemais), so those become one entity with both places' Verses and locations. A place with
//! no such TIPNR place keeps an id of its own (`Name@OpenBible.a1b2c3d`), and is kept only if it
//! has a location. With the same ids, the Passage Guide, the Factbook and the Atlas see one
//! place, not two.
//!
//! **Never kept:** coordinates that come from OpenStreetMap (their licence is ODbL, not CC BY),
//! identifications scored 0 or below (unknown, or judged wrong), geometry, images, comments,
//! descriptions and other sources' links, and places the Bible text does not name in any Verse.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use obs_model::{Entity, EntityKind, PlaceLocation, Resource, ResourceType};
use obs_scripture::{Book, Passage, Standard, Verse, Versification};
use serde::Deserialize;

use crate::dataset::{DatasetError, DatasetOptions, start};
use crate::report::ImportReport;

/// `source_format` of a People & places index converted from OpenBible.info's geocoding data.
pub const OPENBIBLE_GEO_FORMAT: &str = "openbible-geocoding";

/// The `linked_data` key of TIPNR in OpenBible.info's `source.jsonl` (`friendly_id` `tipnr`).
const TIPNR_SOURCE: &str = "s3b25cf";

/// `coordinates_source.type` of coordinates taken from OpenStreetMap (ODbL).
const OPENSTREETMAP: &str = "osm";

/// Dropped-record reasons, counted in the report.
pub const DROPPED_OSM: &str = "location: coordinates from OpenStreetMap (ODbL)";
pub const DROPPED_SCORE: &str = "location: identification scored 0 or below";
pub const DROPPED_NO_VERSE: &str = "place: named in no Verse";
pub const DROPPED_UNLOCATED: &str = "place: not in TIPNR and with no location";
pub const DROPPED_VERSE: &str = "mention: unreadable or outside the Standard versification";

#[derive(Deserialize)]
struct Ancient {
    id: String,
    friendly_id: String,
    #[serde(default)]
    verses: Vec<AncientVerse>,
    #[serde(default)]
    linked_data: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    modern_associations: BTreeMap<String, Association>,
    #[serde(default)]
    translation_name_counts: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct AncientVerse {
    osis: String,
    #[serde(default)]
    alternate_verses: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Association {
    name: String,
    score: i64,
}

#[derive(Deserialize)]
struct Modern {
    id: String,
    #[serde(default)]
    lonlat: Option<String>,
    #[serde(default)]
    coordinates_source: Option<CoordinatesSource>,
}

#[derive(Deserialize)]
struct CoordinatesSource {
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

/// One OpenBible place, read.
struct Place {
    id: String,
    /// The name without OpenBible's number (`Antioch 2` → `Antioch`).
    name: String,
    /// Names to match TIPNR by, lower case.
    match_names: BTreeSet<String>,
    spellings: BTreeSet<String>,
    verses: BTreeSet<Verse>,
    locations: Vec<PlaceLocation>,
}

/// Convert OpenBible.info's `ancient.jsonl` and `modern.jsonl`, matching places to `tipnr`
/// (a People & places index converted from TIPNR) when given.
pub fn import_openbible_geocoding(
    ancient: &str,
    modern: &str,
    tipnr: Option<&Resource>,
    options: &DatasetOptions,
) -> Result<(Resource, ImportReport), DatasetError> {
    let (mut builder, mut report) =
        start(options, ResourceType::PeopleAndPlaces, OPENBIBLE_GEO_FORMAT)?;
    let coordinates = read_modern(modern, &mut builder)?;
    let mut places = Vec::new();
    for (i, raw) in ancient.lines().enumerate() {
        let row = raw.trim_start_matches('\u{feff}').trim();
        if row.is_empty() {
            continue;
        }
        let record: Ancient = serde_json::from_str(row).map_err(|e| DatasetError::Syntax {
            line: i + 1,
            message: format!("ancient place: {e}"),
        })?;
        if let Some(place) = place(record, &coordinates, &mut builder, &mut report) {
            places.push(place);
        }
    }
    if places.is_empty() {
        return Err(DatasetError::NoRecords);
    }
    let matcher = tipnr.map(TipnrPlaces::new);
    // Entity id → (name, places merged into it).
    let mut merged: BTreeMap<String, (String, Vec<Place>)> = BTreeMap::new();
    for place in places {
        let (id, name) = match matcher.as_ref().and_then(|m| m.find(&place)) {
            Some(e) => (e.id.clone(), e.name.clone()),
            None if place.locations.is_empty() => {
                builder.count_dropped(DROPPED_UNLOCATED);
                continue;
            }
            None => (
                format!("{}@OpenBible.{}", place.name.replace(' ', "_"), place.id),
                place.name.clone(),
            ),
        };
        merged
            .entry(id)
            .or_insert_with(|| (name, Vec::new()))
            .1
            .push(place);
    }
    for (id, (name, places)) in merged {
        let mut verses = BTreeSet::new();
        let mut spellings = BTreeSet::new();
        // Modern location (name, coordinates) → best score.
        let mut best: BTreeMap<(String, i32, i32), u16> = BTreeMap::new();
        for p in places {
            verses.extend(p.verses);
            spellings.extend(p.spellings);
            spellings.insert(p.name);
            for l in p.locations {
                let score = best.entry((l.name, l.lon_e6, l.lat_e6)).or_insert(0);
                *score = (*score).max(l.score);
            }
        }
        let display = name.replace('_', " ");
        spellings.remove(&display);
        spellings.remove(&name);
        let mut locations: Vec<PlaceLocation> = best
            .into_iter()
            .map(|((name, lon_e6, lat_e6), score)| PlaceLocation {
                name,
                lon_e6,
                lat_e6,
                score,
            })
            .collect();
        // Most confident first; at equal scores, a location called what the place is called
        // (Jerusalem's `Jerusalem` before its `City of David`).
        locations.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| (a.name != display).cmp(&(b.name != display)))
                .then_with(|| a.cmp(b))
        });
        for v in verses {
            builder.add_mention(&id, Passage::verse(v));
        }
        builder.add_entity(Entity {
            id,
            name,
            kind: EntityKind::Place,
            strongs: None,
            other_strongs: Vec::new(),
            other_names: spellings.into_iter().collect(),
            region: None,
            relations: Vec::new(),
            locations,
        });
    }
    let resource = builder.build().map_err(DatasetError::Invalid)?;
    report.entities = resource.entities.len();
    report.mentions = resource.mentions.len();
    report.dropped_markup = resource.dropped_markup.clone();
    Ok((resource, report))
}

/// Modern location id → its coordinates (millionths of a degree), OpenStreetMap ones left out.
fn read_modern(
    text: &str,
    builder: &mut obs_model::ResourceBuilder,
) -> Result<HashMap<String, (i32, i32)>, DatasetError> {
    let mut out = HashMap::new();
    for (i, raw) in text.lines().enumerate() {
        let row = raw.trim_start_matches('\u{feff}').trim();
        if row.is_empty() {
            continue;
        }
        let syntax = |message: String| DatasetError::Syntax {
            line: i + 1,
            message,
        };
        let m: Modern =
            serde_json::from_str(row).map_err(|e| syntax(format!("modern location: {e}")))?;
        let Some(lonlat) = m.lonlat else {
            continue;
        };
        if m.coordinates_source
            .and_then(|c| c.kind)
            .is_some_and(|k| k == OPENSTREETMAP)
        {
            builder.count_dropped(DROPPED_OSM);
            continue;
        }
        let point = lonlat
            .split_once(',')
            .and_then(|(lon, lat)| Some((micro_degrees(lon, 180)?, micro_degrees(lat, 90)?)))
            .ok_or_else(|| syntax(format!("`{}`: bad lonlat `{lonlat}`", m.id)))?;
        out.insert(m.id, point);
    }
    Ok(out)
}

/// `"36.171743"` → 36171743, if within ±`limit` degrees.
fn micro_degrees(s: &str, limit: i32) -> Option<i32> {
    let v: f64 = s.trim().parse().ok()?;
    if !v.is_finite() || v.abs() > f64::from(limit) {
        return None;
    }
    // Within ±180°, so the rounded value fits an i32.
    #[allow(clippy::cast_possible_truncation)]
    Some((v * 1e6).round() as i32)
}

fn place(
    a: Ancient,
    coordinates: &HashMap<String, (i32, i32)>,
    builder: &mut obs_model::ResourceBuilder,
    report: &mut ImportReport,
) -> Option<Place> {
    let mut verses = BTreeSet::new();
    for v in &a.verses {
        let osis = v.alternate_verses.get("kjv").unwrap_or(&v.osis);
        match parse_verse(osis).filter(|v| Standard.contains(*v)) {
            Some(verse) => {
                verses.insert(verse);
            }
            None => {
                builder.count_dropped(DROPPED_VERSE);
                report
                    .warnings
                    .push(format!("{} ({}): dropped `{osis}`", a.friendly_id, a.id));
            }
        }
    }
    if verses.is_empty() {
        builder.count_dropped(DROPPED_NO_VERSE);
        return None;
    }
    let mut locations = Vec::new();
    for (modern, assoc) in &a.modern_associations {
        let Some(&(lon_e6, lat_e6)) = coordinates.get(modern) else {
            // Left out with its OpenStreetMap coordinates (counted there).
            continue;
        };
        if assoc.score <= 0 {
            builder.count_dropped(DROPPED_SCORE);
            continue;
        }
        let score = u16::try_from(assoc.score.min(i64::from(PlaceLocation::MAX_SCORE)))
            .unwrap_or(PlaceLocation::MAX_SCORE);
        locations.push(PlaceLocation {
            name: assoc.name.clone(),
            lon_e6,
            lat_e6,
            score,
        });
    }
    let name = without_number(&a.friendly_id).to_string();
    let mut match_names: BTreeSet<String> = BTreeSet::new();
    match_names.insert(name.to_lowercase());
    if let Some(link) = a.linked_data.get(TIPNR_SOURCE) {
        let ids = link.get("id").into_iter().chain(
            link.get("ids")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten(),
        );
        for id in ids.filter_map(serde_json::Value::as_str) {
            let n = id.split('@').next().unwrap_or(id);
            match_names.insert(n.replace('_', " ").to_lowercase());
        }
    }
    let spellings: BTreeSet<String> = a.translation_name_counts.keys().cloned().collect();
    match_names.extend(spellings.iter().map(|s| s.to_lowercase()));
    Some(Place {
        id: a.id,
        name,
        match_names,
        spellings,
        verses,
        locations,
    })
}

/// `Antioch 2` → `Antioch`.
fn without_number(friendly: &str) -> &str {
    match friendly.rsplit_once(' ') {
        Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => friendly,
    }
}

/// `Acts.13.14`.
fn parse_verse(s: &str) -> Option<Verse> {
    let mut parts = s.trim().split('.');
    let (Some(book), Some(chapter), Some(verse), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    Some(Verse::new(
        Book::from_osis(book)?,
        chapter.parse().ok()?,
        verse.parse().ok()?,
    ))
}

/// TIPNR's places by lower-case name, with the Verses that mention each.
struct TipnrPlaces<'a> {
    by_name: HashMap<String, Vec<&'a Entity>>,
    verses: HashMap<&'a str, BTreeSet<Verse>>,
}

impl<'a> TipnrPlaces<'a> {
    fn new(index: &'a Resource) -> Self {
        let mut by_name: HashMap<String, Vec<&'a Entity>> = HashMap::new();
        for e in index
            .entities
            .iter()
            .filter(|e| e.kind == EntityKind::Place)
        {
            let names = std::iter::once(&e.name).chain(&e.other_names);
            let mut seen = BTreeSet::new();
            for n in names {
                let n = n.replace('_', " ").to_lowercase();
                if seen.insert(n.clone()) {
                    by_name.entry(n).or_default().push(e);
                }
            }
        }
        let mut verses: HashMap<&'a str, BTreeSet<Verse>> = HashMap::new();
        for m in &index.mentions {
            let set = verses.entry(m.entity.as_str()).or_default();
            let (start, end) = (m.passage.start(), m.passage.end());
            if start == end {
                set.insert(start);
            } else {
                set.extend([start, end]);
            }
        }
        Self { by_name, verses }
    }

    /// The TIPNR place `place` is: called by one of its names and sharing the most Verses with
    /// it (ties go to the lower id, so the build is deterministic). None shares a Verse: none.
    fn find(&self, place: &Place) -> Option<&'a Entity> {
        let mut best: Option<(usize, &'a Entity)> = None;
        for name in &place.match_names {
            for e in self.by_name.get(name).into_iter().flatten() {
                let shared = self
                    .verses
                    .get(e.id.as_str())
                    .map_or(0, |v| v.intersection(&place.verses).count());
                let better = match best {
                    None => shared > 0,
                    Some((n, b)) => shared > n || (shared == n && e.id < b.id),
                };
                if better {
                    best = Some((shared, e));
                }
            }
        }
        best.map(|(_, e)| e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_come_off_names_and_kjv_verses_parse() {
        assert_eq!(without_number("Antioch 2"), "Antioch");
        assert_eq!(without_number("Mount Nebo"), "Mount Nebo");
        assert_eq!(without_number("Jerusalem"), "Jerusalem");
        assert_eq!(
            parse_verse("Acts.13.14"),
            Some(Verse::new(Book::Acts, 13, 14))
        );
        assert_eq!(
            parse_verse("2Tim.3.11"),
            Some(Verse::new(Book::SecondTimothy, 3, 11))
        );
        assert_eq!(parse_verse("Acts.13"), None);
        assert_eq!(micro_degrees("36.171743", 180), Some(36_171_743));
        assert_eq!(micro_degrees("-6.944167", 180), Some(-6_944_167));
        assert_eq!(micro_degrees("95", 90), None);
    }
}
