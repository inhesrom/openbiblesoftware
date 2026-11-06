//! The Atlas (#150): an offline map of the places a Passage names.
//!
//! - **Places** come from the library's People & places indexes: every place an index lists
//!   in the Passage, in the order the Passage first names them, with its Place locations (the
//!   OpenBible.info geocoding in the Starter library gives them, under TIPNR's ids, so a place
//!   is one entity with one Factbook). A place no index locates is listed, not drawn.
//! - **Confidence** of each location is OpenBible.info's score, shown as a [`Confidence`]
//!   level: High (500 and above, as the data's own notes put it), Medium, Low.
//! - **The base map** is built into the app ([`crate::base_map`]): no tile server, no network.
//! - The Tab follows its Link set's Passage like the Cross-references Tool, mapping the whole
//!   chapter of a single Verse ([`atlas_passage`]); clicking a place opens its Factbook. The
//!   search box finds any located place by name.
//!
//! The Factbook's Map section (a place's locations; for a person, the places TIPNR says they
//! lived in) reads the same data.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use obs_model::{EntityKind, PlaceLocation};
use obs_scripture::{Passage, Standard, Verse};

use crate::base_map::GeoBounds;
use crate::factbook::{DirectoryCache, PeopleDirectory};
use crate::people_places::usual_name;
use crate::{Library, Session, WordStatus};

/// People & places indexes whose places carry Place locations, by resource id: the Starter
/// library's OpenBible.info geocoding. The Map section's empty state offers them.
pub const GEOCODING_INDEXES: &[&str] = &["openbible-geo"];

/// Whether a People & places index locates places ([`GEOCODING_INDEXES`]).
pub fn is_geocoding_index(resource_id: &str) -> bool {
    GEOCODING_INDEXES.contains(&resource_id)
}

/// How sure scholarship is of one Place location (from OpenBible.info's score).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// Score 500 and above: the data's notes call this high confidence.
    High,
    /// Score 100 to 499.
    Medium,
    /// Below 100: one of several guesses, or a tentative one.
    Low,
}

impl Confidence {
    /// The lowest scores of [`Confidence::High`] and [`Confidence::Medium`].
    pub const HIGH: u16 = 500;
    pub const MEDIUM: u16 = 100;

    pub fn of(score: u16) -> Self {
        if score >= Self::HIGH {
            Confidence::High
        } else if score >= Self::MEDIUM {
            Confidence::Medium
        } else {
            Confidence::Low
        }
    }

    /// `High confidence`.
    pub fn label(self) -> &'static str {
        match self {
            Confidence::High => "High confidence",
            Confidence::Medium => "Medium confidence",
            Confidence::Low => "Low confidence",
        }
    }
}

/// One Place location, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacePoint {
    /// The modern location's name: `Antioch in Pisidia`, `Konya`.
    pub location: String,
    /// Millionths of a degree, as stored ([`PlacePoint::lon`] and [`PlacePoint::lat`] give
    /// degrees).
    pub lon_e6: i32,
    pub lat_e6: i32,
    pub score: u16,
    pub confidence: Confidence,
}

impl PlacePoint {
    pub fn lon(&self) -> f64 {
        f64::from(self.lon_e6) / 1e6
    }

    pub fn lat(&self) -> f64 {
        f64::from(self.lat_e6) / 1e6
    }
}

impl From<&PlaceLocation> for PlacePoint {
    fn from(l: &PlaceLocation) -> Self {
        PlacePoint {
            location: l.name.clone(),
            lon_e6: l.lon_e6,
            lat_e6: l.lat_e6,
            score: l.score,
            confidence: Confidence::of(l.score),
        }
    }
}

/// A place on a map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPlace {
    /// Its People & places id (its Factbook's Key).
    pub entity: String,
    /// Its usual name: `Antioch`.
    pub name: String,
    /// What tells it from other places of that name (its region), when the index gives one.
    pub region: Option<String>,
    /// Where it may have been, most confident first; empty when no index locates it.
    pub points: Vec<PlacePoint>,
    /// The Verses of the Passage that name it (the Atlas); empty elsewhere.
    pub verses: Vec<Verse>,
}

impl MapPlace {
    /// The most confident location: where the place is drawn.
    pub fn best(&self) -> Option<&PlacePoint> {
        self.points.first()
    }

    /// `Antioch · Antioch in Pisidia · High confidence`.
    pub fn describe(&self) -> String {
        match self.best() {
            Some(p) if p.location != self.name => {
                format!("{} · {} · {}", self.name, p.location, p.confidence.label())
            }
            Some(p) => format!("{} · {}", self.name, p.confidence.label()),
            None => format!("{} · location unknown", self.name),
        }
    }
}

/// Places with the same best location as an earlier one in the list (Pisidia drawn at
/// Pisidian Antioch) are drawn once; this is the list's first place at each point.
pub fn first_at_each_point(places: &[MapPlace]) -> Vec<&MapPlace> {
    let mut seen: BTreeSet<(i32, i32)> = BTreeSet::new();
    places
        .iter()
        .filter(|p| p.best().is_some_and(|b| seen.insert((b.lon_e6, b.lat_e6))))
        .collect()
}

/// The box to show `places` in: around their best locations, at least `degrees` of latitude
/// tall. `None` when none is located.
pub fn bounds_of(places: &[MapPlace], degrees: f64) -> Option<GeoBounds> {
    GeoBounds::around(
        places
            .iter()
            .filter_map(|p| p.best().map(|b| (b.lon(), b.lat()))),
    )
    .map(|b| b.at_least(degrees))
}

/// The Passage an Atlas Tab maps for the Passage it shows: one Verse widens to its chapter (a
/// Bible scrolled to Acts 13:14 maps all of Acts 13); a range or chapter stays as it is.
pub fn atlas_passage(shown: Passage) -> Passage {
    let start = shown.start();
    if shown.end() != start {
        return shown;
    }
    Passage::chapter(&Standard, start.book, start.chapter).unwrap_or(shown)
}

/// The map place for directory entity `id` (places only), naming it in `verses`.
pub fn map_place(dir: &PeopleDirectory, id: &str, verses: Vec<Verse>) -> Option<MapPlace> {
    let entry = dir.get(id)?;
    (entry.entity.kind == EntityKind::Place).then(|| MapPlace {
        entity: entry.entity.id.clone(),
        name: usual_name(&entry.entity),
        region: entry.entity.region.clone(),
        points: entry
            .entity
            .locations
            .iter()
            .map(PlacePoint::from)
            .collect(),
        verses,
    })
}

/// The located places called `query` (or whose names start with it), most often named first.
pub fn search(dir: &PeopleDirectory, query: &str) -> Vec<MapPlace> {
    dir.choose(query)
        .choices
        .iter()
        .filter_map(|card| map_place(dir, &card.entity, Vec::new()))
        .filter(|p| !p.points.is_empty())
        .collect()
}

/// The Factbook's Map for entity `id`: a place with every location it may have had; a person or
/// people group with the places whose records say they lived there.
pub fn factbook_map(dir: &PeopleDirectory, id: &str) -> Vec<MapPlace> {
    let Some(entry) = dir.get(id) else {
        return Vec::new();
    };
    if entry.entity.kind == EntityKind::Place {
        return map_place(dir, id, Vec::new())
            .filter(|p| !p.points.is_empty())
            .into_iter()
            .collect();
    }
    dir.places_lived_in(id)
        .iter()
        .filter_map(|place| map_place(dir, place, Vec::new()))
        .filter(|p| !p.points.is_empty())
        .collect()
}

/// Every place the library's People & places indexes list in `passage`, in the order the
/// Passage first names them (then by name), each once.
#[cfg(not(target_arch = "wasm32"))]
pub fn passage_places(
    library: &Library,
    directory: &DirectoryCache,
    passage: Passage,
) -> Result<Vec<MapPlace>, String> {
    use obs_store::ResourceStore;
    let dir = directory.get(library)?;
    let mut named: BTreeMap<String, BTreeSet<Verse>> = BTreeMap::new();
    let mut errors = Vec::new();
    let mut read_any = false;
    for index in library.of_type(obs_model::ResourceType::PeopleAndPlaces) {
        let mentions = obs_store::ObrReader::open(&index.path).and_then(|r| r.mentions_in(passage));
        let mentions = match mentions {
            Ok(m) => m,
            Err(e) => {
                errors.push(format!("{}: {e}", index.manifest.abbreviation));
                continue;
            }
        };
        read_any = true;
        for m in mentions {
            let verses = named.entry(m.entity).or_default();
            for v in m.passage.verses(&Standard) {
                if passage.contains_verse(v) {
                    verses.insert(v);
                }
            }
        }
    }
    if !read_any && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    let mut places: Vec<MapPlace> = named
        .into_iter()
        .filter_map(|(id, verses)| map_place(&dir, &id, verses.into_iter().collect()))
        .collect();
    places.sort_by(|a, b| {
        a.verses
            .first()
            .cmp(&b.verses.first())
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.entity.cmp(&b.entity))
    });
    Ok(places)
}

#[cfg(target_arch = "wasm32")]
pub fn passage_places(
    _: &Library,
    _: &DirectoryCache,
    _: Passage,
) -> Result<Vec<MapPlace>, String> {
    Err("Opening Resources is not supported here".into())
}

impl Session {
    /// The Atlas search box: located places called `query`, once the People & places
    /// directory is loaded (it loads on the job queue; ask again on a later frame).
    pub fn atlas_search(&mut self, query: &str) -> WordStatus<Vec<MapPlace>> {
        match self.people_directory() {
            WordStatus::Ready(d) => WordStatus::Ready(Arc::new(search(&d, query))),
            WordStatus::Loading => WordStatus::Loading,
            WordStatus::Failed(e) => WordStatus::Failed(e),
        }
    }
}
