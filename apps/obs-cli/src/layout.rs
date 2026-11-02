//! `obs-cli layout`: lay a whole text unit out and report blocks, lines and timing. A plain
//! timing harness until `cargo xtask bench` lands (#33).

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use obs_fonts::ShapeCache;
use obs_layout::{TextSettings, block_paragraph, estimate_height, layout, unit_blocks};
use obs_model::UnitId;
use obs_store::{ObrReader, ResourceStore};

const USAGE: &str =
    "usage: obs-cli layout <file.obr> --unit ID [--width PX] [--size PX] [--runs N]";

pub fn layout_cmd(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let (mut unit, mut width, mut size, mut runs) = (None, 450.0f32, 17.0f32, 2usize);
    let mut it = args[1..].iter();
    while let Some(flag) = it.next() {
        let value = it.next();
        let ok = match (flag.as_str(), value) {
            ("--unit", Some(v)) => {
                unit = Some(v.clone());
                true
            }
            ("--width", Some(v)) => v.parse().map(|v| width = v).is_ok(),
            ("--size", Some(v)) => v.parse().map(|v| size = v).is_ok(),
            ("--runs", Some(v)) => v.parse().map(|v: usize| runs = v.max(1)).is_ok(),
            _ => false,
        };
        if !ok {
            eprintln!("obs-cli layout: bad option `{flag}`\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    let Some(unit_id) = unit.and_then(|u| UnitId::new(u).ok()) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let reader = match ObrReader::open(Path::new(path)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("obs-cli layout: {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let unit = match reader.unit(&unit_id) {
        Ok(Some(u)) => u,
        Ok(None) => {
            eprintln!("obs-cli layout: no unit `{unit_id}`");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("obs-cli layout: {e}");
            return ExitCode::FAILURE;
        }
    };
    let direction = reader.manifest().direction;
    let settings = TextSettings {
        size,
        ..TextSettings::default()
    };
    let cache = ShapeCache::new(400_000);
    let blocks = unit_blocks(&unit);
    for run in 1..=runs {
        let start = Instant::now();
        let (mut lines, mut height, mut estimated) = (0usize, 0.0f32, 0.0f32);
        for b in &blocks {
            let para = block_paragraph(&unit, b, direction, &settings);
            estimated += estimate_height(&para, width);
            let laid = layout(&para, width, &cache);
            lines += laid.lines.len();
            height += laid.height;
        }
        let elapsed = start.elapsed();
        let stats = cache.stats();
        println!(
            "run {run}: {} blocks, {lines} lines, {:.0} px tall (estimated {:.0}, {:+.1}%) in {:.1} ms; shape cache {:.0}% hits ({} entries)",
            blocks.len(),
            height,
            estimated,
            (estimated / height.max(1.0) - 1.0) * 100.0,
            elapsed.as_secs_f64() * 1000.0,
            stats.hit_rate() * 100.0,
            stats.entries
        );
    }
    ExitCode::SUCCESS
}
