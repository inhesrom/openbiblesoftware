//! Open Bible Software desktop app.
//!
//! `openbible [file.obr]` opens the app; `openbible --version` prints its version. `openbible --bench [--obr FILE] [--hz N] [--json OUT]
//! [--interlinear-panes N] [--quick] [--no-shell-scenarios]` runs the headless
//! scroll/fling/jump benchmark and its feature scenarios (`cargo xtask bench`).
//! `openbible --bench --scale [--dir DIR] [--resources N] [--quick] [--json OUT]` runs the
//! scale scenario over a generated library (`cargo xtask bench --scale`, #227).
//!
//! The UI is chosen when the app is built (ADR 0025, rule 2): egui by default, the GPUI UI
//! being built (#371) with `--features gpui`, which then wins if both are compiled in. The GPUI
//! app's `openbible --smoke` opens the window, waits for a frame to be drawn and quits (CI).

#[cfg(not(any(feature = "egui", feature = "gpui")))]
compile_error!("build openbible with the `egui` feature (the default) or the `gpui` feature");

use std::process::ExitCode;
use std::sync::Arc;

#[cfg(feature = "egui")]
use obs_ui_egui::bench::{BenchConfig, run, scale, synthetic_bible};

// The egui app's glue: what its Shell is handed at launch.
#[cfg(feature = "egui")]
#[cfg_attr(feature = "gpui", allow(dead_code))]
mod clipboard;
mod fetch;
#[cfg(feature = "egui")]
#[cfg_attr(feature = "gpui", allow(dead_code))]
mod fonts;
#[cfg(test)]
mod live_tests;
#[cfg(feature = "egui")]
#[cfg_attr(feature = "gpui", allow(dead_code))]
mod system_motion;
// macOS: the menu bar is the window's title bar (#297); elsewhere only `main_viewport` is used.
#[cfg(feature = "egui")]
#[cfg_attr(any(not(target_os = "macos"), feature = "gpui"), allow(dead_code))]
mod title_bar;

#[cfg(feature = "egui")]
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
            // Jumps land at once, as with Reduce motion on (#225).
            "--reduce-motion" => cfg.motion = false,
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

#[cfg(feature = "egui")]
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

#[cfg(feature = "egui")]
/// One process of the scale scenario ([`bench_scale`]), started with its folders' environment.
fn bench_scale_child(
    started: std::time::Instant,
    mode: &str,
    root: &str,
    args: &[String],
) -> ExitCode {
    if let Err(e) = scale::check_scale_env(std::path::Path::new(root)) {
        eprintln!("openbible {mode}: {e}");
        return ExitCode::FAILURE;
    }
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
    if args.first().is_some_and(|a| a == "--version") {
        println!("openbible {}", obs_engine::updates::APP_VERSION);
        return ExitCode::SUCCESS;
    }
    #[cfg(feature = "egui")]
    {
        if args.first().is_some_and(|a| a == "--bench") {
            return bench(&args[1..]);
        }
        if let Some(mode) = args
            .first()
            .filter(|a| *a == "--bench-scale-prepare" || *a == "--bench-scale-run")
        {
            // The folders come from the environment; the folder argument checks it.
            let root = args.get(1).map(String::as_str).unwrap_or_default();
            return bench_scale_child(started, mode, root, args.get(2..).unwrap_or_default());
        }
    }
    start_diagnostics();
    #[cfg(feature = "gpui")]
    return run_gpui(started, &args);
    #[cfg(not(feature = "gpui"))]
    run_egui(&args)
}

/// The egui app: the UI that ships until the cutover (ADR 0025).
#[cfg(feature = "egui")]
#[cfg_attr(feature = "gpui", allow(dead_code))]
fn run_egui(args: &[String]) -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: title_bar::main_viewport(),
        ..Default::default()
    };
    let file = args.first().map(Into::into);
    match eframe::run_native(
        obs_ui_egui::APP_TITLE,
        options,
        Box::new(|_cc| {
            let app = obs_ui_egui::App::with_file(file);
            #[cfg(target_os = "macos")]
            let app = app.with_unified_title_bar(Arc::new(title_bar::OsTitleBar::start()));
            Ok(Box::new(
                app.with_fetch(Arc::new(fetch::HttpFetch::new()))
                    .with_system_language(sys_locale::get_locale())
                    .with_update_checks()
                    .with_system_fonts(Arc::new(fonts::FontdbFonts::system()))
                    .with_system_motion(Arc::new(system_motion::OsMotion::start()))
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

/// The GPUI app being built (ADR 0025, #371): the same Session as the egui app's, in a window
/// that draws an empty Workspace for now. `--smoke` quits once a frame has been drawn.
#[cfg(feature = "gpui")]
fn run_gpui(started: std::time::Instant, args: &[String]) -> ExitCode {
    use obs_ui_gpui::{Options, Outcome};
    let smoke = args.iter().any(|a| a == "--smoke");
    let file = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(std::path::PathBuf::from);
    let mut launch = obs_engine::launch::open_session(file.as_deref());
    launch.session.set_fetch(Arc::new(fetch::HttpFetch::new()));
    launch
        .session
        .get_bibles
        .set_system_language(sys_locale::get_locale());
    obs_engine::launch::start_update_checks(&mut launch.session);
    match obs_ui_gpui::run(launch, Options { smoke }, started) {
        Outcome::Closed => ExitCode::SUCCESS,
        Outcome::SmokePassed { first_frame } => {
            println!(
                "openbible --smoke: a frame was drawn {} ms after start",
                first_frame.as_millis()
            );
            ExitCode::SUCCESS
        }
        Outcome::SmokeFailed(e) => {
            eprintln!("openbible --smoke: {e}");
            ExitCode::FAILURE
        }
    }
}
