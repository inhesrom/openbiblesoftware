//! Help → Credits and the published change records (#67), through the shell's real-event
//! headless harness. The Credits are checked against `ATTRIBUTION.toml` and
//! `starter/sources.toml` as they are on disk.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use obs_engine::{HeaderBox, TabId};
use obs_model::{Manifest, Origin, ResourceType};

use super::*;

fn repo_file(rel: &str) -> toml::Table {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Every `[[asset]]` row of ATTRIBUTION.toml: (path, name).
fn attribution_rows() -> Vec<(String, String)> {
    repo_file("ATTRIBUTION.toml")["asset"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["path"].as_str().unwrap().to_string(),
                a["name"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// Every `[[source]]` of the Starter lock: (resource_id, title, abbreviation, spdx).
fn starter_sources() -> Vec<(String, String, String, String)> {
    repo_file("starter/sources.toml")["source"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            let f = |k: &str| s[k].as_str().unwrap().to_string();
            (f("resource_id"), f("title"), f("abbreviation"), f("spdx"))
        })
        .collect()
}

fn pane_rect(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
}

fn texts_in(h: &Harness, tab: TabId) -> Vec<String> {
    let r = pane_rect(h, tab);
    h.shapes
        .iter()
        .filter(|(s, _)| r.contains(s.center()))
        .map(|(_, t)| t.clone())
        .collect()
}

fn wheel(h: &mut Harness, tab: TabId, dy: f32) {
    let over = pane_rect(h, tab).center();
    h.events.push(Event::PointerMoved(over));
    h.events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, dy),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    h.frame();
}

/// Wheel down through the whole Tab; every text painted in it on the way.
fn scroll_through(h: &mut Harness, tab: TabId) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = texts_in(h, tab).into_iter().collect();
    let mut still = 0;
    for _ in 0..2000 {
        wheel(h, tab, -120.0);
        let before = seen.len();
        seen.extend(texts_in(h, tab));
        still = if seen.len() == before { still + 1 } else { 0 };
        if still >= 20 {
            return seen;
        }
    }
    panic!("the Credits never stopped scrolling");
}

/// Where `text` is painted well inside the Tab, if it is.
fn visible(h: &Harness, tab: TabId, text: &str) -> Option<Rect> {
    let r = pane_rect(h, tab).shrink(30.0);
    h.shapes
        .iter()
        .find(|(s, t)| t == text && r.contains(s.center()))
        .map(|(s, _)| *s)
}

/// Wheel down until `text` is painted inside the Tab, let the scroll settle, and return where.
fn scroll_to(h: &mut Harness, tab: TabId, text: &str) -> Rect {
    for _ in 0..2000 {
        if visible(h, tab, text).is_some() {
            // Wheel scrolling is animated: wait for it to stop before pointing at the text.
            h.frames(30);
            if let Some(p) = visible(h, tab, text) {
                return p;
            }
        }
        wheel(h, tab, -60.0);
    }
    panic!("{text:?} never scrolled into view");
}

/// Click the link painted as `text` in the Tab (at `point` of its rect); the URL it asked
/// to open.
fn click_link(h: &mut Harness, tab: TabId, text: &str, point: fn(Rect) -> Pos2) -> String {
    settle_all(h);
    let before = h.opened.len();
    let at = point(scroll_to(h, tab, text));
    // Away first, so the last link's hover tooltip is gone and cannot cover this one.
    h.events.push(Event::PointerMoved(pos2(2.0, 2.0)));
    h.frames(2);
    h.click(at);
    assert_eq!(
        h.opened.len(),
        before + 1,
        "clicking {text:?} opened nothing"
    );
    h.opened[before].clone()
}

/// Visual evidence for docs/previews/ when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

fn open_credits(h: &mut Harness) -> TabId {
    h.open_menu("Help");
    h.click_text("Credits");
    h.frames(2);
    let tab = h.tab_id("Credits");
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.focused(), Some(tab));
    let t = ws.tab(tab).unwrap();
    assert_eq!(t.kind, TabKind::Credits);
    assert_eq!(t.header_box(), HeaderBox::Keyless);
    tab
}

/// #67 acceptance: every asset row in ATTRIBUTION.toml appears in Credits; so does every
/// Starter Resource, the data built into the app and the third-party crates.
#[test]
fn credits_tab_shows_every_attribution_row_and_every_starter_resource() {
    let mut h = Harness::new();
    let tab = open_credits(&mut h);
    preview(&h, "credits");
    let seen = scroll_through(&mut h, tab);
    let rows = attribution_rows();
    assert!(rows.len() >= 15);
    for (path, name) in &rows {
        assert!(
            seen.contains(name),
            "ATTRIBUTION.toml row {path} ({name:?}) is not in Credits"
        );
    }
    for (id, title, _, _) in starter_sources() {
        assert!(seen.contains(&title), "Starter Resource {id} missing");
    }
    for t in [
        "STARTER LIBRARY",
        "DATA BUILT INTO THE APP",
        "FONTS",
        "TEST DATA IN THE SOURCE CODE",
        "THIRD-PARTY SOFTWARE",
        "OpenBible.info cross-reference votes",
        // TVTMS: "STEP Bible" linked to its site, and the changes noted.
        "STEP Bible, https://www.STEPBible.org",
        "Gentium Plus licence (OFL.txt)",
        "Ezra SIL licence (Licenses.txt)",
    ] {
        assert!(seen.contains(t), "{t:?} missing from {seen:#?}");
    }
    let crates = obs_engine::credits::credits().third_party.crates.len();
    assert!(seen.contains(&format!("Crates ({crates})")));
    // A second Help → Credits focuses the same Tab.
    h.open_menu("Help");
    h.click_text("Credits");
    let n = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Credits)
        .count();
    assert_eq!(n, 1);
}

#[test]
fn credits_show_the_full_font_licences_and_crate_licences() {
    let mut h = Harness::new();
    let tab = open_credits(&mut h);
    let at = scroll_to(&mut h, tab, "Gentium Plus licence (OFL.txt)");
    h.click(at.center());
    h.frames(2);
    let seen = scroll_through(&mut h, tab);
    assert!(
        seen.iter()
            .any(|t| t.contains("SIL OPEN FONT LICENSE Version 1.1")),
        "OFL text not shown"
    );
    let crates = obs_engine::credits::credits().third_party.crates.len();
    let at = scroll_to(&mut h, tab, &format!("Crates ({crates})"));
    h.click(at.center());
    h.frames(2);
    let seen = scroll_through(&mut h, tab);
    for name in ["egui", "egui_tiles", "rusqlite", "harfrust"] {
        assert!(seen.contains(name), "crate {name} missing");
    }
}

#[test]
fn the_tvtms_step_bible_link_opens_in_the_browser() {
    let mut h = Harness::new();
    let tab = open_credits(&mut h);
    // TVTMS's credit, with the URL at its end made a link.
    let credit = "STEP Bible, https://www.STEPBible.org";
    let url_end = |r: Rect| pos2(r.right() - 20.0, r.center().y);
    assert_eq!(
        click_link(&mut h, tab, credit, url_end),
        "https://www.STEPBible.org"
    );
    // The words before it are not the link.
    let before = h.opened.len();
    let at = scroll_to(&mut h, tab, credit);
    h.click(pos2(at.left() + 10.0, at.center().y));
    assert_eq!(h.opened.len(), before);
}

/// A Starter manifest for a lock entry, as `xtask starter` writes it.
fn starter_manifest(id: &str, title: &str, abbreviation: &str, spdx: &str) -> Manifest {
    let mut m = Manifest::new(id, title, abbreviation, ResourceType::Bible, "en");
    m.license = spdx.into();
    m.attribution = format!("{title} attribution.");
    m.changes = Some("Converted.".into());
    m.origin = Origin::Starter;
    m
}

/// #67 acceptance: every CC BY Starter Resource has a CHANGES file linked from its About Tab.
/// The link opens the copy installed beside the `.obr` when there is one, else the copy
/// published with the release; public-domain Starter Resources and imports have none.
#[test]
fn every_cc_by_starter_resource_links_its_changes_record_from_about() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("obs-credits-about-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sources = starter_sources();
    let cc_by: Vec<&(String, String, String, String)> =
        sources.iter().filter(|s| s.3.contains("CC-BY")).collect();
    assert!(cc_by.len() >= 7, "{} CC BY sources", cc_by.len());
    // The first one's record is installed with the library, as the installer would.
    let installed = cc_by[0].0.clone();
    std::fs::write(dir.join(format!("CHANGES-{installed}.md")), "# Changes").unwrap();

    let mut h = Harness::new();
    for (id, title, abbreviation, spdx) in &sources {
        h.shell.st.session.library.add(LibraryEntry {
            manifest: starter_manifest(id, title, abbreviation, spdx),
            path: dir.join(format!("{id}.obr")),
        });
    }
    for (id, _, abbreviation, spdx) in &sources {
        h.shell.run("resource.about", &json!({ "id": id }));
        h.frames(3);
        let about = h.tab_id(&format!("About · {abbreviation}"));
        assert_eq!(h.shell.st.session.workspace.focused(), Some(about));
        let file = format!("CHANGES-{id}.md");
        if !spdx.contains("CC-BY") {
            assert!(
                h.find("Published change record:").is_none() && h.find(&file).is_none(),
                "{id} ({spdx}) shows a change record"
            );
            continue;
        }
        let opened = click_link(&mut h, about, &file, |r| r.center());
        assert!(h.find("Published change record:").is_some(), "{id}");
        if id == "tahot" {
            preview(&h, "about-change-record");
        }
        if *id == installed {
            assert!(opened.starts_with("file:///"), "{id}: {opened}");
            assert!(opened.ends_with(&format!("/{file}")), "{id}: {opened}");
        } else {
            assert_eq!(
                opened,
                format!(
                    "https://github.com/inhesrom/openbiblesoftware/releases/latest/download/{file}"
                ),
                "{id}"
            );
        }
    }
    // An imported CC BY Resource has no published record (we did not make its changes).
    let mut imported = starter_manifest("my-import", "My import", "MINE", "CC-BY-4.0");
    imported.origin = Origin::Imported;
    h.shell.st.session.library.add(LibraryEntry {
        manifest: imported,
        path: dir.join("my-import.obr"),
    });
    h.shell.run("resource.about", &json!({ "id": "my-import" }));
    h.frames(3);
    assert!(h.find("Published change record:").is_none());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}
