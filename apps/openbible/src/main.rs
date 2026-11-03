//! Open Bible Software desktop app.
//!
//! `openbible [file.obr]` opens the app. `openbible --bench [--obr FILE] [--hz N] [--json OUT]
//! [--quick]` runs the headless scroll/fling/jump benchmark (`cargo xtask bench`).

use std::process::ExitCode;
use std::sync::Arc;

use obs_ui_egui::bench::{BenchConfig, run, synthetic_bible};

fn bench(args: &[String]) -> ExitCode {
    let mut cfg = BenchConfig::default();
    let mut json: Option<String> = None;
    let mut obr: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--hz" => cfg.refresh_hz = it.next().and_then(|v| v.parse().ok()).unwrap_or(60.0),
            "--json" => json = it.next().cloned(),
            "--obr" => obr = it.next().cloned(),
            "--quick" => {
                cfg.steady_frames = 120;
                cfg.flings = 1;
                cfg.jumps = 4;
            }
            _ => {}
        }
    }
    // Seeded slowdown for checking that the gate catches regressions.
    if let Some(ms) = std::env::var("OBS_BENCH_SLOWDOWN_MS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        obs_ui_egui::set_layout_slowdown(std::time::Duration::from_secs_f64(ms / 1000.0));
        eprintln!("bench: seeded slowdown of {ms} ms per layout");
    }
    let doc = match obr {
        Some(p) => match obs_ui_egui::ReadingDoc::open(std::path::Path::new(&p)) {
            Ok(d) => Arc::new(d),
            Err(e) => {
                eprintln!("openbible --bench: {p}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => synthetic_bible(),
    };
    let report = run(doc, cfg);
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    println!("{text}");
    if let Some(path) = json
        && let Err(e) = std::fs::write(&path, &text)
    {
        eprintln!("openbible --bench: {path}: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--bench") {
        return bench(&args[1..]);
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(obs_ui_egui::APP_TITLE)
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    let file = args.first().map(Into::into);
    match eframe::run_native(
        obs_ui_egui::APP_TITLE,
        options,
        Box::new(|_cc| Ok(Box::new(obs_ui_egui::App::with_file(file)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("openbible: {e}");
            ExitCode::FAILURE
        }
    }
}
