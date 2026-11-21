//! "Hello engine" (S03 of `docs/plans/mobile-ios.md`): the FFI surface boots the Mobile app
//! over a Bundled set, opens a reader and lays out a phone-sized tile.
//!
//! The fixture test runs everywhere. The ignored one boots against the real Bundled set and
//! writes `target/ios/gen1.png`, Genesis 1 in the BSB at 3×, to look at:
//! `cargo xtask ios bundled-set --from dist/starter`, then
//! `cargo test -p obs-ffi --test hello_engine -- --ignored --nocapture`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use obs_ffi::{MobileApp, Paths, Tile};

/// A fresh folder for one test, and the app's paths in it.
fn paths(name: &str) -> (PathBuf, Paths) {
    let root = std::env::temp_dir().join(format!("obs-ffi-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let at = |p: &str| root.join(p).to_string_lossy().into_owned();
    let paths = Paths {
        library: at("library"),
        user_db: at("user.db"),
        search_cache: at("search"),
        alias_cache: at("aliases"),
        logs: at("logs"),
        downloads: at("downloads"),
    };
    (root, paths)
}

/// Pixels that are neither the page colour (the top-left pixel) nor transparent.
fn inked(tile: &Tile) -> usize {
    let page = &tile.rgba[..4];
    tile.rgba
        .chunks_exact(4)
        .filter(|p| *p != page && p[3] > 0)
        .count()
}

#[test]
fn the_ffi_boots_over_a_bundled_set_and_lays_out_a_phone_tile() {
    let (root, paths) = paths("fixture");
    let bundle = root.join("bundle");
    let mut set = vec![obs_engine::testing::bibles::bsb_john_1()];
    set[0].manifest.origin = obs_model::Origin::Starter;
    obs_engine::testing::write_starter_bundle(&bundle, &set);

    let app = MobileApp::boot(paths, Some(bundle.to_string_lossy().into_owned())).unwrap();
    assert!(app.problems().is_empty(), "{:?}", app.problems());
    assert_eq!(app.resource_ids(), ["bsb"]);
    assert_eq!(app.reader_resource(0).as_deref(), Some("bsb"));

    let reader = app
        .open_reader("bsb".into(), Some("John 1:1".into()))
        .unwrap();
    let tile = app.layout_tile(reader, 0.0, 390.0, 256.0, 3.0).unwrap();
    assert_eq!((tile.width, tile.height), (1170, 768));
    assert_eq!(tile.rgba.len(), 1170 * 768 * 4);
    assert!(inked(&tile) > 10_000, "{} inked pixels", inked(&tile));

    let err = app.open_reader("nasb".into(), None).unwrap_err();
    assert_eq!(err.to_string(), "no Resource nasb in the library");
    assert!(app.layout_tile(7, 0.0, 390.0, 256.0, 3.0).is_err());
    assert_eq!(obs_ffi::engine_version(), obs_engine::updates::APP_VERSION);
}

/// The Bundled set `cargo xtask ios bundled-set` copies, or the whole Starter library.
fn real_bundle() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    ["apps/ios/Bundled/starter", "dist/starter"]
        .into_iter()
        .map(|d| root.join(d))
        .find(|d| d.join("starter-manifest.json").is_file())
}

fn write_png(path: &Path, tile: &Tile) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut png = png::Encoder::new(file, tile.width, tile.height);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    // Every pixel is opaque, so premultiplied and straight alpha are the same bytes.
    assert!(tile.rgba.chunks_exact(4).all(|p| p[3] == 255));
    png.write_header()
        .unwrap()
        .write_image_data(&tile.rgba)
        .unwrap();
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

#[test]
#[ignore = "needs apps/ios/Bundled/starter from `cargo xtask ios bundled-set` (or dist/starter)"]
fn genesis_1_in_the_bsb_at_3x_from_the_real_bundled_set() {
    let Some(bundle) = real_bundle() else {
        eprintln!("no apps/ios/Bundled/starter or dist/starter; skipped");
        return;
    };
    let (_, paths) = paths("gen1");
    let started = Instant::now();
    let app = MobileApp::boot(paths, Some(bundle.to_string_lossy().into_owned())).unwrap();
    let boot = started.elapsed();
    assert!(app.problems().is_empty(), "{:?}", app.problems());
    assert!(app.resource_ids().iter().any(|id| id == "bsb"));

    let started = Instant::now();
    let reader = app.open_reader("bsb".into(), Some("Gen 1".into())).unwrap();
    let open = started.elapsed();
    let started = Instant::now();
    let tile = app.layout_tile(reader, 0.0, 390.0, 768.0, 3.0).unwrap();
    let cold = started.elapsed();
    assert_eq!((tile.width, tile.height), (1170, 2304));
    assert!(inked(&tile) > 50_000, "{} inked pixels", inked(&tile));
    let png = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ios/gen1.png");
    write_png(&png, &tile);

    // A full-width 3× phone tile (390 × 256 pt), again and again: laid out and rastered
    // once, so each is the composite (plan A4: > 8 ms puts option B in doubt).
    let mut warm: Vec<Duration> = (0..20)
        .map(|i| {
            let top = 256.0 * (i % 3) as f32;
            let started = Instant::now();
            let t = app.layout_tile(reader, top, 390.0, 256.0, 3.0).unwrap();
            assert_eq!((t.width, t.height), (1170, 768));
            started.elapsed()
        })
        .collect();
    warm.sort();
    eprintln!(
        "boot {:.1} ms; open Gen 1 {:.1} ms; first 1170x2304 tile {:.1} ms; \
         1170x768 tiles: median {:.2} ms, max {:.2} ms; wrote {}",
        ms(boot),
        ms(open),
        ms(cold),
        ms(warm[warm.len() / 2]),
        ms(warm[warm.len() - 1]),
        png.display()
    );
}
