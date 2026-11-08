//! Open Bible Software desktop app.
//!
//! `openbible [file.obr]` opens the app. `openbible --bench [--obr FILE] [--hz N] [--json OUT]
//! [--interlinear-panes N] [--quick] [--no-shell-scenarios]` runs the headless
//! scroll/fling/jump benchmark and its feature scenarios (`cargo xtask bench`).
//! `openbible --bench --scale [--dir DIR] [--resources N] [--quick] [--json OUT]` runs the
//! scale scenario over a generated library (`cargo xtask bench --scale`, #227).

use std::process::ExitCode;
use std::sync::Arc;

use obs_ui_egui::bench::{BenchConfig, run, scale, synthetic_bible};

mod clipboard;
mod fetch;
mod fonts;
#[cfg(test)]
mod live_tests;

fn bench(args: &[String]) -> ExitCode {
    let mut cfg = BenchConfig::default();
    let mut json: Option<String> = None;
    let mut obr: Option<String> = None;
    let mut scale_dir: Option<String> = None;
    let mut resources = scale::SCALE_RESOURCES;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--scale" => {
                scale_dir.get_or_insert_with(|| "target/bench-scale".into());
            }
            "--dir" => scale_dir = it.next().cloned(),
            "--resources" => {
                resources = it.next().and_then(|v| v.parse().ok()).unwrap_or(resources);
            }
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
    if let Some(dir) = scale_dir {
        return bench_scale(
            &scale::ScaleDirs::new(dir),
            resources,
            &cfg,
            json.as_deref(),
        );
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

/// The scale scenario (#227): the library generated (or kept from an earlier run), the app
/// launched over it to reset the Last session, then the measured run, each in a process of
/// its own whose folders are all inside the scale folder.
fn bench_scale(
    dirs: &scale::ScaleDirs,
    resources: usize,
    cfg: &BenchConfig,
    json: Option<&str>,
) -> ExitCode {
    let library = match scale::write_scale_library(dirs, resources) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("openbible --bench --scale: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "bench --scale: {} Resources, {:.1} GB in {} ({} in {:.0} s)",
        library.resources,
        library.bytes as f64 / 1e9,
        dirs.library().display(),
        if library.made { "made" } else { "kept" },
        library.seconds
    );
    let Ok(exe) = std::env::current_exe() else {
        eprintln!("openbible --bench --scale: cannot find this program");
        return ExitCode::FAILURE;
    };
    let child = |mode: &str, out: Option<&std::path::Path>| {
        let mut c = std::process::Command::new(&exe);
        c.arg(mode).arg(&dirs.root);
        c.arg("--hz").arg(cfg.refresh_hz.to_string());
        if cfg.tool_frames < BenchConfig::default().tool_frames {
            c.arg("--quick");
        }
        if let Some(out) = out {
            c.arg("--json").arg(out);
        }
        c.envs(dirs.env());
        c.status()
    };
    match child("--bench-scale-prepare", None) {
        Ok(s) if s.success() => {}
        other => {
            eprintln!("openbible --bench --scale: the launch before the run failed: {other:?}");
            return ExitCode::FAILURE;
        }
    }
    let out = dirs.root.join("report.json");
    match child("--bench-scale-run", Some(&out)) {
        Ok(s) if s.success() => {}
        other => {
            eprintln!("openbible --bench --scale: the run failed: {other:?}");
            return ExitCode::FAILURE;
        }
    }
    let text = std::fs::read_to_string(&out).unwrap_or_default();
    println!("{text}");
    if let Some(path) = json
        && let Err(e) = std::fs::write(path, &text)
    {
        eprintln!("openbible --bench --scale: {path}: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// One process of the scale scenario ([`bench_scale`]), started with its folders' environment.
fn bench_scale_child(started: std::time::Instant, mode: &str, args: &[String]) -> ExitCode {
    let mut cfg = BenchConfig::default();
    let mut json: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--hz" => cfg.refresh_hz = it.next().and_then(|v| v.parse().ok()).unwrap_or(60.0),
            "--json" => json = it.next().cloned(),
            "--quick" => {
                cfg.tool_frames = 30;
                cfg.tool_steps = 2;
            }
            _ => {}
        }
    }
    if mode == "--bench-scale-prepare" {
        return match scale::prepare_launch() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("openbible --bench --scale: {e}");
                ExitCode::FAILURE
            }
        };
    }
    // Another word for the Library search, to measure a rarer or commoner one.
    let search = std::env::var("OBS_SCALE_LIBRARY_SEARCH")
        .unwrap_or_else(|_| scale::LIBRARY_SEARCH.to_string());
    let report = scale::run(started, &cfg, &search);
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    if let Some(path) = json
        && let Err(e) = std::fs::write(&path, &text)
    {
        eprintln!("openbible --bench --scale: {path}: {e}");
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
    // A test build panics here on request, to check the Crash report end to end: after a
    // windowing-library warning quoting typed text, as egui-winit's for an IME event it
    // ignores, and with text the code put in the panic message, unquoted. Neither text may
    // reach the log file or the report.
    #[cfg(debug_assertions)]
    if std::env::var_os("OBS_FORCE_PANIC").is_some() {
        log::warn!(
            target: "egui_winit",
            "ignoring Preedit(\"PRIVATE_NOTE\", Some((0, 99)))'s range because it is invalid"
        );
        #[allow(clippy::panic)]
        {
            panic!("forced panic (OBS_FORCE_PANIC): invalid note PRIVATE_NOTE");
        }
    }
}

fn main() -> ExitCode {
    // Cold start is measured from here (the scale scenario, #227).
    let started = std::time::Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--bench") {
        return bench(&args[1..]);
    }
    if let Some(mode) = args
        .first()
        .filter(|a| *a == "--bench-scale-prepare" || *a == "--bench-scale-run")
    {
        // The folder argument is for `ps`; the folders come from the environment.
        return bench_scale_child(started, mode, args.get(2..).unwrap_or_default());
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
