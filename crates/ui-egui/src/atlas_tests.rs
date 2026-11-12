//! The Atlas (#150) through the shell's real-event harness: Acts 13 draws the places of
//! Paul's first journey where they are, a click on a place opens its Factbook (whose Map shows
//! where it was and how sure that is), the search box finds a place, and the Tab follows its
//! Link set's Bible to another chapter.

use std::path::{Path, PathBuf};

use obs_engine::testing::{bibles, openbible_geo, tipnr_acts_13};
use obs_engine::{Guide, Library, TabId};
use obs_model::Key;
use obs_scripture::{Book, Verse};

use super::*;

struct Files(PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// TIPNR's Acts 13 places, the geocoding matched to them and (with `bible`) a little KJV with
/// Acts 13:4 and Acts 21:7, as `.obr` files in a fresh library.
fn harness(name: &str, bible: bool) -> (Harness, Files) {
    let root = std::env::temp_dir().join(format!("obs-ui-atlas-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let files = Files(root.clone());
    let tipnr = tipnr_acts_13();
    let geo = openbible_geo(Some(&tipnr));
    let mut resources = Vec::new();
    if bible {
        resources.push(bibles::bible(
            "kjv",
            "KJV",
            &[
                (
                    Verse::new(Book::Acts, 13, 4),
                    "So they, being sent forth by the Holy Ghost, departed unto Seleucia; and \
                     from thence they sailed to Cyprus.",
                ),
                (
                    Verse::new(Book::Acts, 21, 7),
                    "And when we had finished our course from Tyre, we came to Ptolemais.",
                ),
            ],
            None,
            &[],
        ));
    }
    resources.push(tipnr);
    resources.push(geo);
    bibles::write_library(&root.join("library"), &resources);
    let mut h = Harness::new();
    let s = &mut h.shell.st.session;
    s.library = Library::new();
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.set_alias_cache_dir(root.join("cache"));
    h.shell.reset_layout();
    h.frames(2);
    (h, files)
}

fn pane(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    let id = tiles.find_pane(&tab).unwrap();
    tiles.rect(id).unwrap()
}

/// Every place `text` is painted in Tab `tab`, left to right.
fn all_in(h: &Harness, tab: TabId, text: &str) -> Vec<Rect> {
    let r = pane(h, tab);
    let mut found: Vec<Rect> = h
        .shapes
        .iter()
        .filter(|(s, t)| t == text && r.contains(s.center()))
        .map(|(s, _)| *s)
        .collect();
    found.sort_by(|a, b| a.left().total_cmp(&b.left()));
    found
}

fn until(h: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..600 {
        if done(h) {
            return;
        }
        h.paced_frame();
    }
    panic!("never saw {what}: {:?}", h.texts());
}

/// A place's label on the map: the leftmost of its texts (the list is at the right).
fn label(h: &Harness, tab: TabId, name: &str) -> Rect {
    all_in(h, tab, name)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no {name} on the map: {:?}", h.texts()))
}

/// The acceptance check: opening Acts 13 shows the places of Paul's first journey, each
/// where it is (Cyrene west, Jerusalem south-east, Iconium north), and a click on a place
/// opens its Factbook, whose Map says where it was and how sure that is.
#[test]
fn acts_13_shows_the_first_journey_and_a_place_opens_its_factbook() {
    let (mut h, _files) = harness("acts-13", false);
    let atlas = TabId(
        h.shell
            .run(
                "tool.atlas",
                &json!({ "passage": "Acts 13", "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    let journey = [
        "Seleucia",
        "Cyprus",
        "Salamis",
        "Paphos",
        "Perga",
        "Pamphylia",
        "Iconium",
    ];
    until(&mut h, "the journey's places", |h| {
        journey.iter().all(|p| !all_in(h, atlas, p).is_empty())
            && all_in(h, atlas, "Antioch").len() >= 2
    });
    let status = h
        .texts()
        .into_iter()
        .find(|t| t.starts_with("Acts 13 · "))
        .map(str::to_string);
    assert_eq!(
        status.as_deref(),
        Some("Acts 13 · 12 places"),
        "{:?}",
        h.texts()
    );
    // The labels sit where the places are: west to east, south to north.
    let (cyrene, jerusalem, iconium, paphos) = (
        label(&h, atlas, "Cyrene"),
        label(&h, atlas, "Jerusalem"),
        label(&h, atlas, "Iconium"),
        label(&h, atlas, "Paphos"),
    );
    assert!(cyrene.left() < paphos.left() && paphos.left() < jerusalem.left());
    assert!(iconium.top() < paphos.top() && paphos.top() < jerusalem.top());
    // The list beside the map gives each place's location and confidence.
    let texts = h.texts();
    assert!(
        texts.contains(&"Antioch on the Orontes · High confidence"),
        "{texts:?}"
    );
    assert!(texts.contains(&"New Paphos · High confidence"), "{texts:?}");
    preview(&h, "atlas-acts-13-kittest");

    // A click on Iconium's marker (just left of its label) opens its Factbook.
    let iconium = label(&h, atlas, "Iconium");
    let marker = pos2(iconium.left() - 8.0, iconium.center().y);
    h.click(marker);
    h.frames(3);
    let ws = &h.shell.session().workspace;
    let factbook = ws.focused().unwrap();
    let t = ws.tab(factbook).unwrap();
    assert_eq!(t.kind, TabKind::Guide(Guide::Factbook));
    assert_eq!(
        t.entry_history.current(),
        Some(Key::Headword("Iconium@Act.13.51-2Ti".into()))
    );
    until(&mut h, "Iconium's Map", |h| {
        !all_in(h, factbook, "Konya · High confidence").is_empty()
            && !all_in(h, factbook, "Map").is_empty()
    });
    preview(&h, "factbook-iconium-map-kittest");
}

/// Pisidia is drawn where Pisidian Antioch is. Picked in the list, its ringed marker is on
/// top of the city's, and a click on it opens Pisidia's Factbook, not Antioch's.
#[test]
fn a_picked_place_sharing_a_spot_is_the_one_clicked() {
    let (mut h, _files) = harness("coincident", false);
    let atlas = TabId(
        h.shell
            .run(
                "tool.atlas",
                &json!({ "passage": "Acts 13", "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    until(&mut h, "the place list", |h| {
        !all_in(h, atlas, "Factbook").is_empty()
    });
    // Wheel the list down to Pisidia, its last place.
    let list = all_in(&h, atlas, "Factbook")[0].center();
    h.events.push(Event::PointerMoved(list));
    for _ in 0..200 {
        if !all_in(&h, atlas, "Pisidia").is_empty() {
            break;
        }
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -40.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        });
        h.frame();
    }
    h.frames(2);
    // Only the list names Pisidia until it is picked.
    let row = all_in(&h, atlas, "Pisidia");
    assert_eq!(row.len(), 1, "{:?}", h.texts());
    h.click(row[0].center());
    h.frames(3);
    until(&mut h, "Pisidia on the map", |h| {
        all_in(h, atlas, "Pisidia").len() == 2
    });
    // Its label is right of its marker.
    let pisidia = label(&h, atlas, "Pisidia");
    h.click(pos2(pisidia.left() - 8.0, pisidia.center().y));
    h.frames(3);
    let ws = &h.shell.session().workspace;
    let t = ws.tab(ws.focused().unwrap()).unwrap();
    assert_eq!(t.kind, TabKind::Guide(Guide::Factbook));
    assert_eq!(
        t.entry_history.current(),
        Some(Key::Headword("Pisidia@Act.13.14-".into()))
    );
}

/// The search box finds a place by name; picking it shows it on the map, ringed.
#[test]
fn the_search_box_finds_a_place_and_shows_it() {
    let (mut h, _files) = harness("search", false);
    let atlas = TabId(
        h.shell
            .run(
                "tool.atlas",
                &json!({ "passage": "Acts 13", "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    until(&mut h, "the map", |h| {
        !all_in(h, atlas, "Iconium").is_empty()
    });
    let field = h.find("Find a place…").unwrap();
    h.click(field.center());
    h.events.push(Event::Text("Acco".into()));
    h.frames(2);
    until(&mut h, "the search result", |h| {
        !all_in(h, atlas, "Acco (Acre)").is_empty()
    });
    let result = all_in(&h, atlas, "Acco (Acre)")[0];
    h.click(result.center());
    h.frames(3);
    // Acco is not in Acts 13, but now it is on the map, picked.
    until(&mut h, "Acco on the map", |h| {
        !all_in(h, atlas, "Acco").is_empty()
    });
    let picked = h.shell.st.tabs.get(&atlas).and_then(|t| t.map.as_ref());
    assert!(picked.is_some());
    preview(&h, "atlas-search-acco-kittest");
}

/// The Atlas follows its Link set: the Bible moving to Acts 21:7 moves the map to Acts 21
/// (Ptolemais, which TIPNR calls Acco, and Cyprus in 21:3); one Verse maps its whole
/// chapter.
#[test]
fn the_atlas_follows_its_link_sets_bible_to_another_chapter() {
    let (mut h, _files) = harness("link", true);
    let bible = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("kjv".into()))
        .unwrap()
        .id;
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": "Acts 13:4" }));
    h.shell
        .run("tab.link", &json!({ "tab": bible.0, "set": "A" }));
    let atlas = TabId(
        h.shell
            .run("tool.atlas", &json!({ "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.shell
        .run("tab.link", &json!({ "tab": atlas.0, "set": "A" }));
    h.frames(2);
    until(&mut h, "Acts 13's places", |h| {
        h.texts().contains(&"Acts 13 · 12 places")
    });
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": "Acts 21:7" }));
    until(&mut h, "Acts 21's place", |h| {
        h.texts().contains(&"Acts 21 · 2 places") && !all_in(h, atlas, "Acco").is_empty()
    });
}

/// #293: the wheel zooms the map as much at every Scroll speed; a Scroll speed is for
/// scrolling, not for the Atlas's zoom.
#[test]
fn the_wheel_zooms_the_map_the_same_at_every_scroll_speed() {
    let (mut h, _files) = harness("scroll-speed", false);
    let atlas = TabId(
        h.shell
            .run(
                "tool.atlas",
                &json!({ "passage": "Acts 13", "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    until(&mut h, "Iconium on the map", |h| {
        !all_in(h, atlas, "Iconium").is_empty()
    });
    let scale = |h: &Harness| {
        (h.shell.st.tabs.get(&atlas))
            .and_then(|t| t.map.as_ref())
            .and_then(|m| m.view())
            .expect("the map's view")
            .scale
    };
    let over = label(&h, atlas, "Iconium").center();
    let mut zooms = Vec::new();
    for speed in [1.0, 0.5, 2.0] {
        h.shell.run("view.scroll_speed", &json!({ "value": speed }));
        h.frames(2);
        for unit in [egui::MouseWheelUnit::Line, egui::MouseWheelUnit::Point] {
            let before = scale(&h);
            h.events.push(Event::PointerMoved(over));
            h.events.push(Event::MouseWheel {
                unit,
                delta: vec2(
                    0.0,
                    if unit == egui::MouseWheelUnit::Line {
                        1.0
                    } else {
                        40.0
                    },
                ),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
            h.frames(60);
            zooms.push((speed, unit, scale(&h) / before));
        }
    }
    for (speed, unit, zoom) in &zooms {
        assert!(
            *zoom > 1.05,
            "{speed}× {unit:?}: the wheel zooms in ({zoom})"
        );
        let at_one = zooms.iter().find(|z| z.0 == 1.0 && z.1 == *unit).unwrap().2;
        assert!(
            (zoom - at_one).abs() < 1e-3,
            "{speed}× {unit:?}: {zoom}, at 1×: {at_one}"
        );
    }
}

/// Arimathea's seven possible locations, all within 30 km of one another.
const ARIMATHEA: [&str; 7] = [
    "Al Bira",
    "Al Ram",
    "Beit Rima",
    "Nabi Samwil",
    "Ramallah",
    "Ramat Rahel",
    "Rantis",
];

/// GUIDES-18 (#304): a place's Factbook Map fits its possible locations, not a regional map
/// around them, so they spread across the map apart from one another, each named, rather
/// than sitting in one cluster with most of their labels left out.
#[test]
fn a_places_factbook_map_fits_its_several_locations() {
    let (mut h, _files) = harness("arimathea", false);
    let factbook = TabId(
        h.shell
            .run(
                "guide.factbook",
                &json!({ "name": "Arimathea", "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    until(&mut h, "Arimathea's Map", |h| {
        !all_in(h, factbook, "Ramallah · Medium confidence").is_empty()
            || h.texts().iter().any(|t| t.starts_with("Ramallah · "))
    });
    h.frames(4);
    preview(&h, "factbook-arimathea-map-kittest");
    // Each location is named on the map (the list under it names them with a confidence).
    let labels: Vec<Rect> = ARIMATHEA
        .iter()
        .map(|name| {
            all_in(&h, factbook, name)
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("{name} is not named on the map: {:?}", h.texts()))
        })
        .collect();
    // They spread over most of the map's height (240 points), from Rantis and Beit Rima in the
    // north-west to Ramat Rahel in the south.
    let top = labels.iter().map(|r| r.top()).fold(f32::MAX, f32::min);
    let bottom = labels.iter().map(|r| r.bottom()).fold(f32::MIN, f32::max);
    let left = labels.iter().map(|r| r.left()).fold(f32::MAX, f32::min);
    let right = labels.iter().map(|r| r.right()).fold(f32::MIN, f32::max);
    assert!(
        bottom - top > 120.0,
        "the locations' labels are {} points apart from top to bottom: {labels:?}",
        bottom - top
    );
    assert!(
        right - left > 150.0,
        "the locations' labels are {} points apart from left to right: {labels:?}",
        right - left
    );
    // No two labels overlap.
    for (i, a) in labels.iter().enumerate() {
        for b in &labels[i + 1..] {
            assert!(!a.intersects(*b), "{a:?} overlaps {b:?}");
        }
    }
}

/// GUIDES-18 (#304), with Jerusalem's four possible locations as the Starter's geocoding
/// gives them: the Factbook's Map of a place fits them, so Shufat, 5 km north, is well apart
/// from the city, where a regional map (7 degrees tall) drew all four on one spot.
#[test]
fn jerusalems_factbook_map_fits_its_four_locations() {
    use crate::shell::atlas_tab::{LOCATIONS_SPAN, MINI_MAP_HEIGHT, MINI_MAP_SPAN, fit};
    use obs_engine::{Confidence, MapPlace, PlacePoint};
    let point = |location: &str, lon: f64, lat: f64| PlacePoint {
        location: location.into(),
        // Degrees as stored, in millionths.
        #[allow(clippy::cast_possible_truncation)]
        lon_e6: (lon * 1e6).round() as i32,
        #[allow(clippy::cast_possible_truncation)]
        lat_e6: (lat * 1e6).round() as i32,
        score: 1000,
        confidence: Confidence::High,
    };
    let jerusalem = MapPlace {
        entity: "Jerusalem@Jos.10.1-Rev".into(),
        name: "Jerusalem".into(),
        region: None,
        points: vec![
            point("Jerusalem", 35.234_167, 31.776_667),
            point("City of David", 35.235_556, 31.773_611),
            point("Mount Zion", 35.228_611, 31.771_667),
            point("Shufat", 35.223_220, 31.820_140),
        ],
        verses: Vec::new(),
    };
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, MINI_MAP_HEIGHT));
    let size = (f64::from(rect.width()), f64::from(rect.height()));
    let apart = |view: obs_engine::base_map::MapView| {
        let [a, b] = [&jerusalem.points[0], &jerusalem.points[3]]
            .map(|p| view.to_screen(p.lon(), p.lat(), size));
        (a.0 - b.0).hypot(a.1 - b.1)
    };
    let regional = fit(&[&jerusalem], rect, true, MINI_MAP_SPAN);
    let fitted = fit(&[&jerusalem], rect, true, LOCATIONS_SPAN);
    assert!(apart(regional) < 2.0, "{}", apart(regional));
    assert!(apart(fitted) > 25.0, "{}", apart(fitted));
    // Every location is on the map.
    let visible = fitted.visible(size);
    for p in &jerusalem.points {
        assert!(
            visible.contains(p.lon(), p.lat()),
            "{} is off the map",
            p.location
        );
    }

    // Drawn, each is named, though Jerusalem, the City of David and Mount Zion are a few
    // points apart even fitted (the City of David went unnamed, #304).
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(rect),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        let mut view = fitted;
        crate::shell::atlas_tab::map_canvas(
            ui,
            rect,
            &mut view,
            &crate::shell::atlas_tab::MapScene {
                places: &[&jerusalem],
                selected: None,
                emphasized: &|_| false,
                all_points: true,
                interactive: false,
                description: "Map: Jerusalem",
            },
        );
    });
    // No renderer takes the font atlas.
    output.textures_delta.clear();
    let mut texts = Vec::new();
    for clipped in &output.shapes {
        collect(&clipped.shape, &mut texts);
    }
    let names: Vec<&str> = texts.iter().map(|(_, t)| t.as_str()).collect();
    for p in &jerusalem.points {
        assert!(
            names.contains(&p.location.as_str()),
            "{} is not named: {names:?}",
            p.location
        );
    }
    for (i, (a, ta)) in texts.iter().enumerate() {
        for (b, tb) in &texts[i + 1..] {
            assert!(!a.intersects(*b), "{ta} {a:?} overlaps {tb} {b:?}");
        }
    }
}

/// The Screenshot gallery's Factbook Map of a place with several possible locations (#304):
/// Arimathea's seven, from the geocoding excerpt put in the library.
pub(super) fn factbook_maps() -> Vec<super::a11y_audit::Overlay> {
    vec![(
        "Factbook Map: a place's several locations",
        |h| {
            let library = &mut h.shell.st.session.library;
            let dir = library.directory().expect("a library folder").to_path_buf();
            bibles::write_library(&dir, &[openbible_geo(None)]);
            library.add_file(&dir.join("openbible-geo.obr")).unwrap();
            h.frames(2);
            h.shell.run(
                "guide.factbook",
                &json!({ "name": "Arimathea", "split": true }),
            );
            super::missing_resource::settle(h);
        },
        |h| ARIMATHEA.iter().all(|name| h.find(name).is_some()),
    )]
}
