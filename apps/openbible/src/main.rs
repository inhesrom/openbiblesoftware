//! Open Bible Software desktop app.
//!
//! `openbible [file.obr]` opens the app. `openbible --bench [--obr FILE] [--hz N] [--json OUT]
//! [--interlinear-panes N] [--quick] [--no-shell-scenarios]` runs the headless
//! scroll/fling/jump benchmark and its feature scenarios (`cargo xtask bench`).

use std::process::ExitCode;
use std::sync::Arc;

use obs_ui_egui::bench::{BenchConfig, run, synthetic_bible};

mod clipboard;
mod fetch;
mod fonts;
#[cfg(test)]
mod live_tests;

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
            "--interlinear-panes" => {
                cfg.interlinear_panes = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
            }
            "--quick" => {
                cfg.steady_frames = 120;
                cfg.flings = 1;
                cfg.jumps = 4;
                cfg.tool_frames = 30;
                cfg.tool_steps = 2;
            }
            "--no-shell-scenarios" => cfg.shell_scenarios = false,
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
    // Seeded synchronous feature work that fits in a frame: only the 2 ms budget catches it.
    if let Some(ms) = std::env::var("OBS_BENCH_FEATURE_SLOWDOWN_MS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        cfg.feature_slowdown = std::time::Duration::from_secs_f64(ms / 1000.0);
        eprintln!("bench: seeded slowdown of {ms} ms per frame in each feature's Tab");
    }
    // Seeded synchronous work only in each action's frame (a catalog query changing, say).
    if let Some(ms) = std::env::var("OBS_BENCH_ACTION_SLOWDOWN_MS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        cfg.action_slowdown = std::time::Duration::from_secs_f64(ms / 1000.0);
        eprintln!("bench: seeded slowdown of {ms} ms in each feature action's own frame");
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

/// Log files and Crash reports in the Log folder (#219), before anything else runs. The Log
/// level saved in the User database is applied when the app attaches it.
fn start_diagnostics() {
    use obs_engine::diagnostics;
    let Some(dir) = diagnostics::default_log_dir() else {
        return;
    };
    if let Err(e) = diagnostics::start_logging(&dir, obs_engine::LogLevel::default()) {
        eprintln!("openbible: no log file: {e}");
    }
    diagnostics::install_panic_hook(dir);
    // A test build panics here on request, to check the Crash report end to end.
    #[cfg(debug_assertions)]
    if std::env::var_os("OBS_FORCE_PANIC").is_some() {
        #[allow(clippy::panic)]
        {
            panic!("forced panic (OBS_FORCE_PANIC)");
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--bench") {
        return bench(&args[1..]);
    }
    start_diagnostics();
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
        Box::new(|_cc| {
            Ok(Box::new(
                obs_ui_egui::App::with_file(file)
                    .with_fetch(Arc::new(fetch::HttpFetch::new()))
                    .with_system_fonts(Arc::new(fonts::FontdbFonts::system()))
                    .with_rich_clipboard(Arc::new(clipboard::SystemClipboard::default()))
                    .with_automation_from_env(),
            ))
        }),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("openbible: {e}");
            ExitCode::FAILURE
        }
    }
}
