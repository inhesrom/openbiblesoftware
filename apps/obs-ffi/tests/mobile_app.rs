//! The FFI surface (`docs/dev/mobile-core.md`) as Swift sees it: boot over a Bundled set,
//! readers, navigation, frames and tiles, events.
//!
//! The fixture tests run everywhere. The ignored one boots against the real Bundled set and
//! writes `target/ios/gen1.png`, Genesis 1 in the BSB at 3×, to look at:
//! `cargo xtask ios bundled-set --from dist/starter`, then
//! `cargo test -p obs-ffi --test mobile_app -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use obs_ffi::{
    Changed, MobileApp, MobileError, Observer, SystemAppearance, Tile, TilePixels, Viewport,
};

/// A fresh folder for one test.
fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-ffi-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A booted app over a Bundled set of the BSB's Isaiah 40 and John 1.
fn app(name: &str) -> (PathBuf, Arc<MobileApp>) {
    let root = root(name);
    let bundle = root.join("bundle");
    let mut set = vec![obs_engine::testing::bibles::bsb_john_1()];
    set[0].manifest.origin = obs_model::Origin::Starter;
    obs_engine::testing::write_starter_bundle(&bundle, &set);
    let app = MobileApp::boot(s(&root.join("app")), Some(s(&bundle)), None).unwrap();
    assert!(app.problems().unwrap().is_empty(), "{:?}", app.problems());
    (root, app)
}

const PHONE: Viewport = Viewport {
    width: 390.0,
    height: 700.0,
    scale: 3.0,
};
const LIGHT: SystemAppearance = SystemAppearance { dark: false };

/// Pixels that are neither the page colour (the top-left pixel) nor transparent.
fn inked(tile: &Tile) -> usize {
    let rgba = &tile.rgba.0;
    let page: [u8; 4] = rgba[..4].try_into().unwrap();
    (rgba.as_chunks::<4>().0.iter())
        .filter(|p| **p != page && p[3] > 0)
        .count()
}

#[test]
fn the_ffi_boots_reads_and_draws_a_phone_tile() {
    let (root, app) = app("fixture");
    let ids: Vec<String> = (app.library().unwrap().into_iter())
        .map(|r| r.resource_id)
        .collect();
    assert_eq!(ids, ["bsb"]);
    let excluded = app.backup_excluded().unwrap();
    assert_eq!(
        excluded,
        [
            s(&root.join("app/library")),
            s(&root.join("app/search")),
            s(&root.join("app/aliases"))
        ]
    );
    let reader = app.reader(0).unwrap();
    assert_eq!(
        (reader.resource_id.as_str(), reader.location.as_str()),
        ("bsb", "Isaiah 40:1")
    );
    let at = app.navigate(0, "Jn 1:3".into()).unwrap();
    assert_eq!(at.location, "John 1:3");
    assert!(at.can_go_back);
    let f = app.frame(0, 0.0, PHONE, LIGHT).unwrap();
    assert_eq!(f.location, "John 1:3");
    let key = f.tiles.iter().find(|t| t.height >= 100.0).unwrap().key;
    let tile = app.tile(0, key, 3.0).unwrap().unwrap().tile;
    assert_eq!(tile.width, 1170);
    assert_eq!(tile.rgba.0.len(), (tile.width * tile.height * 4) as usize);
    assert!(inked(&tile) > 5_000, "{} inked pixels", inked(&tile));
    assert_eq!(app.back(0).unwrap().location, "Isaiah 40:1");

    // Errors in words: a field's error is a Reference error.
    let err = app.open(0, "nasb".into(), None).unwrap_err();
    assert_eq!(err.to_string(), "no Resource nasb in the library");
    match app.navigate(0, "Hezekiah 4".into()).unwrap_err() {
        MobileError::Reference { message } => assert!(message.contains("Hezekiah"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert!(app.frame(7, 0.0, PHONE, LIGHT).is_err());
    assert!(app.tile(0, key, 0.0).is_err());
    assert_eq!(obs_ffi::engine_version(), obs_engine::updates::APP_VERSION);
    assert_eq!(
        obs_ffi::text_size_for_content_size("UICTContentSizeCategoryL".into()),
        20.0
    );
    drop(app);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A tile's bytes lowered as `Vec<u8>` lowers them (per item) and as [`TilePixels`] does,
/// and the time each took.
fn lower_both(bytes: &[u8]) -> ((Vec<u8>, Duration), (Vec<u8>, Duration)) {
    let started = Instant::now();
    let vec = <Vec<u8> as uniffi::Lower<obs_ffi::UniFfiTag>>::lower(bytes.to_vec());
    let per_item = started.elapsed();
    let started = Instant::now();
    let bulk = <TilePixels as uniffi::Lower<obs_ffi::UniFfiTag>>::lower(TilePixels(bytes.to_vec()));
    let bulk_took = started.elapsed();
    (
        (vec.destroy_into_vec(), per_item),
        (bulk.destroy_into_vec(), bulk_took),
    )
}

#[test]
fn a_3x_tile_crosses_as_bulk_bytes() {
    let (_root, app) = app("bulk");
    let f = app.frame(0, 0.0, PHONE, LIGHT).unwrap();
    let key = (f.tiles.iter())
        .max_by(|a, b| a.height.total_cmp(&b.height))
        .unwrap()
        .key;
    let tile = app.tile(0, key, 3.0).unwrap().unwrap().tile;
    assert_eq!(tile.width, 1170);
    let mut per_item = Duration::MAX;
    let mut bulk = Duration::MAX;
    for _ in 0..5 {
        let ((a, ta), (b, tb)) = lower_both(&tile.rgba.0);
        assert_eq!(a, b, "the same wire bytes");
        per_item = per_item.min(ta);
        bulk = bulk.min(tb);
    }
    eprintln!(
        "{}x{} tile ({} bytes): Vec<u8> per item {per_item:?}, TilePixels {bulk:?}",
        tile.width,
        tile.height,
        tile.rgba.0.len()
    );
    assert!(
        bulk * 4 < per_item,
        "bulk {bulk:?} against per item {per_item:?}"
    );
}

#[derive(Default)]
struct Heard(Mutex<Vec<Changed>>);

impl Observer for Heard {
    fn changed(&self, change: Changed) {
        self.0.lock().unwrap().push(change);
    }
}

#[test]
fn the_pump_thread_tells_the_observer_what_changed() {
    let (_root, app) = app("observer");
    let heard = Arc::new(Heard::default());
    app.set_observer(heard.clone()).unwrap();
    app.navigate(0, "John 1:14".into()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let wanted =
        |h: &[Changed]| h.contains(&Changed::Reader { reader: 0 }) && h.contains(&Changed::History);
    while !wanted(&heard.0.lock().unwrap()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let h = heard.0.lock().unwrap().clone();
    assert!(wanted(&h), "{h:?}");
    assert_eq!(app.history().unwrap()[0].visits[0].label, "John 1:14 · BSB");
    assert!(
        app.poll_changed().unwrap().is_empty(),
        "the thread had them"
    );
}

/// The Bundled set `cargo xtask ios bundled-set` copies, or the whole Starter library.
fn real_bundle() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    ["apps/ios/Bundled/starter", "dist/starter"]
        .into_iter()
        .map(|d| root.join(d))
        .find(|d| d.join("starter-manifest.json").is_file())
}

/// The frame's tiles from `top` down, `height` points of them, as one image.
fn page(app: &MobileApp, frame: &obs_ffi::ReaderFrame, height: f64, scale: f32) -> Tile {
    let width = (PHONE.width * scale).round() as usize;
    let rows = (height * f64::from(scale)).round() as usize;
    let mut rgba = vec![0u8; width * rows * 4];
    for place in &frame.tiles {
        let y = ((place.y - frame.position) * f64::from(scale)).round() as isize;
        if y >= rows as isize || y + (place.height * scale) as isize <= 0 {
            continue;
        }
        let Some(t) = app.tile(0, place.key, scale).unwrap() else {
            continue;
        };
        for row in 0..t.tile.height as isize {
            let to = y + row;
            if to < 0 || to >= rows as isize {
                continue;
            }
            let from = row as usize * t.tile.width as usize * 4;
            let at = to as usize * width * 4;
            rgba[at..at + width * 4].copy_from_slice(&t.tile.rgba.0[from..from + width * 4]);
        }
    }
    Tile {
        width: width as u32,
        height: rows as u32,
        rgba: TilePixels(rgba),
    }
}

fn write_png(path: &Path, tile: &Tile) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut png = png::Encoder::new(file, tile.width, tile.height);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    // Every pixel is opaque, so premultiplied and straight alpha are the same bytes.
    png.write_header()
        .unwrap()
        .write_image_data(&tile.rgba.0)
        .unwrap();
}

#[test]
#[ignore = "needs apps/ios/Bundled/starter from `cargo xtask ios bundled-set` (or dist/starter)"]
fn genesis_1_in_the_bsb_at_3x_from_the_real_bundled_set() {
    let bundle = real_bundle().expect("no Bundled set: run `cargo xtask ios bundled-set`");
    let root = root("gen1");
    let app = MobileApp::boot(s(&root.join("app")), Some(s(&bundle)), None).unwrap();
    assert!(app.problems().unwrap().is_empty(), "{:?}", app.problems());
    assert_eq!(app.reader(0).unwrap().location, "Genesis 1:1");
    let viewport = Viewport {
        height: 768.0,
        ..PHONE
    };
    let f = app.frame(0, 0.0, viewport, LIGHT).unwrap();
    let image = page(&app, &f, 768.0, 3.0);
    assert!(inked(&image) > 50_000, "{} inked pixels", inked(&image));
    let png = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ios/gen1.png");
    write_png(&png, &image);
    eprintln!("wrote {} ({}x{})", png.display(), image.width, image.height);
    drop(app);
    std::fs::remove_dir_all(&root).unwrap();
}
