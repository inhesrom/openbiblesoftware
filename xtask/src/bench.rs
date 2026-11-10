//! `cargo xtask bench`: run the headless reading-pane benchmark and its feature scenarios
//! (#183) in release mode and fail if any number is past its threshold in `xtask/bench.toml`
//! (#19), or a feature scenario listed there did not run.
//!
//! `cargo xtask bench --scale` runs the scale scenario (#227) instead: the app over a
//! generated library of 2,000 Resources, held to `[scale]` in `xtask/bench.toml`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Thresholds {
    pub refresh_hz: f32,
    pub steady: ScrollLimits,
    pub fling: ScrollLimits,
    pub jumps: JumpLimits,
    pub budget: BudgetLimits,
    /// Feature scenarios (#183) by name; every one listed must be in the report.
    pub features: BTreeMap<String, FeatureLimits>,
    pub scale: ScaleLimits,
}

/// The scale scenario's budgets (#227).
#[derive(Debug, Deserialize)]
pub struct ScaleLimits {
    /// Resources the library must have: the scenario fails on a smaller one.
    pub resources: usize,
    pub max_cold_start_ms: f64,
    pub max_library_filter_ms: f64,
    /// The Library Tab scrolled and typed into, as a feature scenario.
    pub library: FeatureLimits,
    pub max_peak_rss_mib: f64,
    pub min_panels: usize,
}

/// What `openbible --bench --scale` reports (`obs_ui_egui::bench::scale::ScaleReport`).
#[derive(Debug, Deserialize)]
pub struct ScaleReport {
    pub resources: usize,
    pub cold_start_ms: f64,
    pub launch_ms: f64,
    pub library_filter_ms_max: f64,
    pub library_filter_ms_p50: f64,
    pub library: Option<Scenario>,
    pub get_resources_filter_ms_max: f64,
    pub get_resources: Option<Scenario>,
    pub library_search: String,
    pub library_search_ms: f64,
    pub library_search_resources: usize,
    pub library_search_hits: usize,
    pub passage_guide_ms: f64,
    pub passage_guide_moved_ms: f64,
    pub panels: usize,
    pub rss_mib: f64,
    pub peak_rss_mib: f64,
    #[serde(default)]
    pub rss_after: Vec<(String, f64)>,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct FeatureLimits {
    pub max_missed_pct: f64,
    /// Synchronous work per frame, p99 (#19: 2 ms): the feature's UI-thread work, which can
    /// be over budget while every frame still fits in the refresh interval.
    pub max_sync_p99_ms: f64,
    /// Frames an action may take to show its result (0: its own frame).
    pub max_settle_frames: Option<usize>,
    pub max_placeholder_frames: Option<usize>,
    /// Milliseconds an action may take to show its result, from its frame starting (#299: a
    /// Text size change in every open Tab, 100 ms).
    pub max_action_ms: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ScrollLimits {
    pub max_missed_pct: f64,
    pub max_placeholder_frames: usize,
}

#[derive(Debug, Deserialize)]
pub struct JumpLimits {
    pub max_missed_pct: f64,
    pub max_origin_settle_frames: usize,
    pub max_all_settle_frames: usize,
}

#[derive(Debug, Deserialize)]
pub struct BudgetLimits {
    pub max_sync_layout_p99_ms: f64,
    pub max_text_gpu_mib: f64,
}

#[derive(Debug, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub missed_pct: f64,
    pub placeholder_frames: usize,
    pub cpu_p99_ms: f64,
    #[serde(default)]
    pub cpu_p50_ms: f64,
    /// Synchronous work per frame, p99: the reading panes' layout, or a Shell-driven
    /// scenario's feature Tab.
    #[serde(default)]
    pub sync_layout_p99_ms: Option<f64>,
    /// Feature scenarios: most frames an action took to show its result.
    #[serde(default)]
    pub settle_frames_max: Option<usize>,
    /// Feature scenarios timed in milliseconds: longest an action took to show its result.
    #[serde(default)]
    pub action_ms_max: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct Report {
    pub scenarios: Vec<Scenario>,
    pub sync_layout_p99_ms: f64,
    pub jump_origin_settle_frames_max: usize,
    pub jump_all_settle_frames_max: usize,
    pub text_gpu_mib: f64,
    /// Find in Resource (#175): matches drawn over every pane each frame, and the time the
    /// whole-document find took (reported, not a threshold: it runs on a worker).
    #[serde(default)]
    pub find_matches: usize,
    #[serde(default)]
    pub find_ms: f64,
    /// Feature scenarios that could not be set up or run.
    #[serde(default)]
    pub feature_errors: Vec<String>,
}

/// Every threshold the report breaks, as messages.
pub fn violations(report: &Report, t: &Thresholds) -> Vec<String> {
    let mut out = Vec::new();
    let mut check = |ok: bool, what: String| {
        if !ok {
            out.push(what);
        }
    };
    for s in &report.scenarios {
        let (missed, placeholders) = match s.name.as_str() {
            "steady" => (
                t.steady.max_missed_pct,
                Some(t.steady.max_placeholder_frames),
            ),
            "fling" => (t.fling.max_missed_pct, Some(t.fling.max_placeholder_frames)),
            "jumps" => (t.jumps.max_missed_pct, None),
            _ => continue,
        };
        check(
            s.missed_pct <= missed,
            format!(
                "{}: {:.2}% missed frames (max {missed}%)",
                s.name, s.missed_pct
            ),
        );
        if let Some(max) = placeholders {
            check(
                s.placeholder_frames <= max,
                format!(
                    "{}: {} placeholder frames (max {max})",
                    s.name, s.placeholder_frames
                ),
            );
        }
    }
    check(
        report.jump_origin_settle_frames_max <= t.jumps.max_origin_settle_frames,
        format!(
            "jumps: origin settled in {} frames (max {})",
            report.jump_origin_settle_frames_max, t.jumps.max_origin_settle_frames
        ),
    );
    check(
        report.jump_all_settle_frames_max <= t.jumps.max_all_settle_frames,
        format!(
            "jumps: all panes settled in {} frames (max {})",
            report.jump_all_settle_frames_max, t.jumps.max_all_settle_frames
        ),
    );
    check(
        report.sync_layout_p99_ms <= t.budget.max_sync_layout_p99_ms,
        format!(
            "UI-thread layout p99 {:.2} ms (max {})",
            report.sync_layout_p99_ms, t.budget.max_sync_layout_p99_ms
        ),
    );
    check(
        report.text_gpu_mib <= t.budget.max_text_gpu_mib,
        format!(
            "text GPU memory {:.1} MiB (max {})",
            report.text_gpu_mib, t.budget.max_text_gpu_mib
        ),
    );
    for e in &report.feature_errors {
        check(false, format!("feature scenario failed: {e}"));
    }
    for (name, limits) in &t.features {
        let Some(s) = report.scenarios.iter().find(|s| s.name == *name) else {
            check(false, format!("{name}: not measured"));
            continue;
        };
        for m in feature_violations(name, s, limits) {
            check(false, m);
        }
    }
    out
}

/// The feature budgets `limits` holds `name`'s scenario `s` to.
fn feature_violations(name: &str, s: &Scenario, limits: &FeatureLimits) -> Vec<String> {
    let mut out = Vec::new();
    let mut check = |ok: bool, what: String| {
        if !ok {
            out.push(what);
        }
    };
    check(
        s.missed_pct <= limits.max_missed_pct,
        format!(
            "{name}: {:.2}% missed frames (max {}%)",
            s.missed_pct, limits.max_missed_pct
        ),
    );
    match s.sync_layout_p99_ms {
        Some(ms) => check(
            ms <= limits.max_sync_p99_ms,
            format!(
                "{name}: synchronous work p99 {ms:.2} ms a frame (max {})",
                limits.max_sync_p99_ms
            ),
        ),
        None => check(false, format!("{name}: no synchronous work measured")),
    }
    if let Some(max) = limits.max_settle_frames {
        match s.settle_frames_max {
            Some(n) => check(
                n <= max,
                format!("{name}: an action took {n} frames to show (max {max})"),
            ),
            None => check(false, format!("{name}: no settle frames measured")),
        }
    }
    if let Some(max) = limits.max_action_ms {
        match s.action_ms_max {
            Some(ms) => check(
                ms <= max,
                format!("{name}: an action took {ms:.1} ms to show (max {max})"),
            ),
            None => check(false, format!("{name}: no action time measured")),
        }
    }
    if let Some(max) = limits.max_placeholder_frames {
        check(
            s.placeholder_frames <= max,
            format!(
                "{name}: {} placeholder frames (max {max})",
                s.placeholder_frames
            ),
        );
    }
    out
}

/// Every `[scale]` budget the scale report breaks, as messages.
pub fn scale_violations(r: &ScaleReport, t: &ScaleLimits) -> Vec<String> {
    let mut out: Vec<String> = r
        .errors
        .iter()
        .map(|e| format!("scale step failed: {e}"))
        .collect();
    let mut check = |ok: bool, what: String| {
        if !ok {
            out.push(what);
        }
    };
    check(
        r.resources >= t.resources,
        format!(
            "scale: the library has {} Resources (needs {})",
            r.resources, t.resources
        ),
    );
    check(
        r.cold_start_ms <= t.max_cold_start_ms,
        format!(
            "scale: cold start {:.0} ms (max {})",
            r.cold_start_ms, t.max_cold_start_ms
        ),
    );
    check(
        r.library_filter_ms_max <= t.max_library_filter_ms,
        format!(
            "scale: the Library Tool filtered in {:.1} ms (max {})",
            r.library_filter_ms_max, t.max_library_filter_ms
        ),
    );
    // A platform whose resident set cannot be read reports 0: not measured is not met.
    check(
        r.peak_rss_mib > 0.0,
        "scale: peak RSS not measured on this platform".into(),
    );
    check(
        r.peak_rss_mib <= t.max_peak_rss_mib,
        format!(
            "scale: peak RSS {:.0} MiB (max {:.1} MiB)",
            r.peak_rss_mib, t.max_peak_rss_mib
        ),
    );
    check(
        r.panels >= t.min_panels,
        format!(
            "scale: {} Panels open when memory was measured (needs {})",
            r.panels, t.min_panels
        ),
    );
    match &r.library {
        Some(s) => out.extend(feature_violations("scale library", s, &t.library)),
        None => out.push("scale library: not measured".into()),
    }
    out
}

fn thresholds(root: &Path) -> Result<Thresholds> {
    toml::from_str(
        &std::fs::read_to_string(root.join("xtask/bench.toml"))
            .context("cannot read xtask/bench.toml")?,
    )
    .context("cannot parse xtask/bench.toml")
}

/// `cargo xtask bench --scale [--dir DIR] [--resources N] [--quick]`: the scale scenario
/// (#227). The library is generated under `DIR` (default `target/bench-scale`) the first time
/// and kept for later runs; it needs about 22 GB of disk, nearly none on a filesystem that
/// clones files (APFS, Btrfs, XFS).
pub fn run_scale(root: &Path, args: &[String]) -> Result<()> {
    let thresholds = thresholds(root)?;
    let limits = &thresholds.scale;
    let mut dir = root.join("target/bench-scale");
    let mut extra: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--scale" => {}
            "--dir" => dir = it.next().context("--dir needs a folder")?.into(),
            "--resources" => {
                extra.push(a.clone());
                extra.push(it.next().context("--resources needs a number")?.clone());
            }
            "--quick" => extra.push(a.clone()),
            other => bail!("bench --scale: unknown option {other}"),
        }
    }
    let json = root.join("target/bench-scale-report.json");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(&cargo)
        .args([
            "run",
            "--release",
            "--locked",
            "--quiet",
            "-p",
            "openbible",
            "--",
            "--bench",
            "--scale",
            "--hz",
        ])
        .arg(thresholds.refresh_hz.to_string())
        .arg("--dir")
        .arg(&dir)
        .args(&extra)
        .arg("--json")
        .arg(&json)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .status()
        .context("could not run the scale bench")?;
    if !status.success() {
        bail!("the scale bench exited with {status}");
    }
    let report: ScaleReport = serde_json::from_str(&std::fs::read_to_string(&json)?)
        .context("cannot parse the scale bench report")?;
    eprintln!(
        "bench --scale at {} Hz, {} Resources:",
        thresholds.refresh_hz, report.resources
    );
    eprintln!(
        "  cold start {:.0} ms (launch {:.0} ms); Library filter max {:.1} ms, median {:.1} ms",
        report.cold_start_ms,
        report.launch_ms,
        report.library_filter_ms_max,
        report.library_filter_ms_p50
    );
    for s in report.library.iter().chain(&report.get_resources) {
        eprintln!(
            "  {:<16} missed {:>5.2}%  frame CPU p50 {:>5.2} ms, p99 {:>5.2} ms  sync p99 {:>5.2} ms",
            s.name,
            s.missed_pct,
            s.cpu_p50_ms,
            s.cpu_p99_ms,
            s.sync_layout_p99_ms.unwrap_or(0.0)
        );
    }
    eprintln!(
        "  Get Resources filter max {:.1} ms; Library search {:?}: {} hits in {} Resources, {:.0} ms",
        report.get_resources_filter_ms_max,
        report.library_search,
        report.library_search_hits,
        report.library_search_resources,
        report.library_search_ms
    );
    eprintln!(
        "  Passage Guide {:.0} ms opened, {:.0} ms moved; {} Panels; RSS {:.0} MiB, peak {:.0} MiB",
        report.passage_guide_ms,
        report.passage_guide_moved_ms,
        report.panels,
        report.rss_mib,
        report.peak_rss_mib
    );
    let steps: Vec<String> = (report.rss_after.iter())
        .map(|(step, mib)| format!("{step} {mib:.0}"))
        .collect();
    eprintln!("  RSS after each step, MiB: {}", steps.join(", "));
    let v = scale_violations(&report, limits);
    if v.is_empty() {
        eprintln!("bench --scale: all thresholds met");
        return Ok(());
    }
    for m in &v {
        eprintln!("bench --scale: REGRESSION {m}");
    }
    bail!("{} scale threshold(s) exceeded", v.len())
}

pub fn run(root: &Path) -> Result<()> {
    let thresholds = thresholds(root)?;
    let json = root.join("target/bench-report.json");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(&cargo)
        .args([
            "run",
            "--release",
            "--locked",
            "--quiet",
            "-p",
            "openbible",
            "--",
            "--bench",
            "--hz",
        ])
        .arg(thresholds.refresh_hz.to_string())
        .arg("--json")
        .arg(&json)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .status()
        .context("could not run the bench")?;
    if !status.success() {
        bail!("the bench exited with {status}");
    }
    let report: Report = serde_json::from_str(&std::fs::read_to_string(&json)?)
        .context("cannot parse the bench report")?;
    eprintln!("bench at {} Hz, 4 panes:", thresholds.refresh_hz);
    for s in &report.scenarios {
        let settle = s
            .settle_frames_max
            .map_or(String::new(), |n| format!("  settle {n} frames"));
        let sync = s
            .sync_layout_p99_ms
            .map_or(String::new(), |ms| format!("  sync p99 {ms:>5.2} ms"));
        let action = s
            .action_ms_max
            .map_or(String::new(), |ms| format!("  action max {ms:.1} ms"));
        eprintln!(
            "  {:<16} missed {:>5.2}%  placeholder frames {:>3}  frame CPU p50 {:>5.2} ms, p99 {:>5.2} ms{sync}{settle}{action}",
            s.name, s.missed_pct, s.placeholder_frames, s.cpu_p50_ms, s.cpu_p99_ms
        );
    }
    eprintln!(
        "  jumps settle: origin {} frames, all panes {} frames; UI layout p99 {:.2} ms; text GPU {:.1} MiB",
        report.jump_origin_settle_frames_max,
        report.jump_all_settle_frames_max,
        report.sync_layout_p99_ms,
        report.text_gpu_mib
    );
    eprintln!(
        "  find: {} matches drawn over each pane; whole-document find {:.1} ms (worker)",
        report.find_matches, report.find_ms
    );
    let v = violations(&report, &thresholds);
    if v.is_empty() {
        eprintln!("bench: all thresholds met");
        return Ok(());
    }
    for m in &v {
        eprintln!("bench: REGRESSION {m}");
    }
    bail!("{} performance threshold(s) exceeded", v.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thresholds() -> Thresholds {
        toml::from_str(include_str!("../bench.toml")).unwrap()
    }

    fn scenario(
        name: &str,
        missed_pct: f64,
        placeholders: usize,
        settle: Option<usize>,
    ) -> Scenario {
        Scenario {
            name: name.into(),
            missed_pct,
            placeholder_frames: placeholders,
            cpu_p50_ms: 1.0,
            cpu_p99_ms: 2.0,
            sync_layout_p99_ms: Some(1.0),
            settle_frames_max: settle,
            action_ms_max: Some(10.0),
        }
    }

    /// A report meeting every threshold, the feature scenarios (#183) included.
    fn report(steady_missed: f64, placeholders: usize, sync_p99: f64) -> Report {
        let mut scenarios = vec![
            scenario("steady", steady_missed, placeholders, None),
            scenario("fling", 0.0, 0, None),
            scenario("jumps", 2.0, 20, None),
        ];
        for name in thresholds().features.keys() {
            scenarios.push(scenario(name, 0.0, 0, Some(0)));
        }
        Report {
            scenarios,
            sync_layout_p99_ms: sync_p99,
            jump_origin_settle_frames_max: 1,
            jump_all_settle_frames_max: 2,
            text_gpu_mib: 4.0,
            find_matches: 0,
            find_ms: 0.0,
            feature_errors: Vec::new(),
        }
    }

    #[test]
    fn a_good_report_passes() {
        assert_eq!(
            violations(&report(0.0, 0, 1.2), &thresholds()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn regressions_are_named() {
        let v = violations(&report(3.5, 12, 4.0), &thresholds());
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(v[0].starts_with("steady: 3.50% missed frames"));
        assert!(v[1].starts_with("steady: 12 placeholder frames"));
        assert!(v[2].starts_with("UI-thread layout p99 4.00 ms"));
    }

    /// Every feature the issue lists has a scenario with a budget (#183).
    #[test]
    fn every_heavy_feature_has_a_budgeted_scenario() {
        let t = thresholds();
        let names: Vec<&str> = t.features.keys().map(String::as_str).collect();
        for feature in [
            "interlinear",
            "reading-layout",
            "atlas",
            "search-grid",
            "search-chart",
            "exegetical-guide",
            "topic-guide",
            "find",
            "filters",
            "get-resources",
            "resize",
            "text-size",
            "text-size-tools",
            "resource-text-size",
        ] {
            assert!(
                names.contains(&feature),
                "{feature} has no budget: {names:?}"
            );
            assert!(t.features[feature].max_missed_pct <= 1.0, "{feature}");
            assert!(t.features[feature].max_sync_p99_ms <= 2.0, "{feature}");
        }
    }

    #[test]
    fn feature_regressions_and_missing_scenarios_fail_the_gate() {
        let mut r = report(0.0, 0, 1.2);
        r.scenarios.retain(|s| s.name != "atlas");
        for s in &mut r.scenarios {
            match s.name.as_str() {
                "exegetical-guide" => s.missed_pct = 4.0,
                "find" => s.settle_frames_max = Some(5),
                "interlinear" => s.placeholder_frames = 3,
                _ => {}
            }
        }
        r.feature_errors
            .push("topic-guide: never saw the Prayer topic".into());
        let v = violations(&r, &thresholds());
        for expected in [
            "feature scenario failed: topic-guide: never saw the Prayer topic",
            "atlas: not measured",
            "exegetical-guide: 4.00% missed frames",
            "find: an action took 5 frames to show",
            "interlinear: 3 placeholder frames",
        ] {
            assert!(
                v.iter().any(|m| m.starts_with(expected)),
                "{expected}: {v:?}"
            );
        }
        assert_eq!(v.len(), 5, "{v:?}");
    }

    /// A Text size change (#299) shows in every open Tab within 100 ms of the key press, in
    /// reading Tabs and in Text Comparison and Search Tabs alike; slower, or not timed, fails.
    #[test]
    fn a_text_size_change_slower_than_100_ms_fails_the_gate() {
        let t = thresholds();
        for name in ["text-size", "text-size-tools", "resource-text-size"] {
            assert_eq!(t.features[name].max_action_ms, Some(100.0), "{name}");
        }
        let mut r = report(0.0, 0, 1.2);
        for s in &mut r.scenarios {
            match s.name.as_str() {
                "text-size" => s.action_ms_max = Some(530.0),
                "text-size-tools" => s.action_ms_max = None,
                _ => {}
            }
        }
        assert_eq!(
            violations(&r, &t),
            [
                "text-size: an action took 530.0 ms to show (max 100)",
                "text-size-tools: no action time measured",
            ]
        );
    }

    fn scale_report() -> ScaleReport {
        ScaleReport {
            resources: 2000,
            cold_start_ms: 900.0,
            launch_ms: 700.0,
            library_filter_ms_max: 6.0,
            library_filter_ms_p50: 1.0,
            library: Some(scenario("library", 0.0, 0, Some(1))),
            get_resources_filter_ms_max: 6.0,
            get_resources: Some(scenario("get-resources", 0.0, 0, Some(1))),
            library_search: "lord".into(),
            library_search_ms: 800.0,
            library_search_resources: 240,
            library_search_hits: 190_000,
            passage_guide_ms: 1800.0,
            passage_guide_moved_ms: 100.0,
            panels: 6,
            rss_mib: 400.0,
            peak_rss_mib: 400.0,
            rss_after: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// The scale gate holds #227's criteria: cold start at most twice #19's floor-machine
    /// target, the Library Tool filtering in 50 ms, peak RSS at most 500 MB with 6 Panels.
    #[test]
    fn the_scale_gate_holds_the_issue_targets() {
        let t = thresholds().scale;
        assert_eq!(t.resources, 2000);
        assert!((t.max_cold_start_ms - 2.0 * 1500.0).abs() < f64::EPSILON);
        assert!((t.max_library_filter_ms - 50.0).abs() < f64::EPSILON);
        // 500 MB, in MiB.
        assert!((t.max_peak_rss_mib - 500e6 / 1_048_576.0).abs() < 0.05);
        assert_eq!(t.min_panels, 6);
        assert_eq!(scale_violations(&scale_report(), &t), Vec::<String>::new());
        let mut r = scale_report();
        r.resources = 120;
        r.cold_start_ms = 3200.0;
        r.library_filter_ms_max = 61.0;
        r.peak_rss_mib = 951.0;
        r.panels = 3;
        if let Some(s) = r.library.as_mut() {
            s.sync_layout_p99_ms = Some(4.1);
        }
        r.errors
            .push("passage guide: never saw every section".into());
        assert_eq!(
            scale_violations(&r, &t),
            [
                "scale step failed: passage guide: never saw every section",
                "scale: the library has 120 Resources (needs 2000)",
                "scale: cold start 3200 ms (max 3000)",
                "scale: the Library Tool filtered in 61.0 ms (max 50)",
                "scale: peak RSS 951 MiB (max 476.8 MiB)",
                "scale: 3 Panels open when memory was measured (needs 6)",
                "scale library: synchronous work p99 4.10 ms a frame (max 2)",
            ]
        );
        r.library = None;
        assert!(
            scale_violations(&r, &t)
                .iter()
                .any(|m| m == "scale library: not measured")
        );
        // Where the resident set cannot be read (0), the memory criterion fails.
        let mut r = scale_report();
        r.peak_rss_mib = 0.0;
        assert_eq!(
            scale_violations(&r, &t),
            ["scale: peak RSS not measured on this platform"]
        );
    }

    /// 3 ms of synchronous work in every frame fits a 60 Hz frame (no missed frames) but is
    /// over #19's 2 ms budget, so it fails the gate; a feature that did not measure its
    /// synchronous work fails too.
    #[test]
    fn sync_work_over_budget_within_a_frame_fails_the_gate() {
        let mut r = report(0.0, 0, 1.2);
        for s in &mut r.scenarios {
            match s.name.as_str() {
                "atlas" => s.sync_layout_p99_ms = Some(3.0),
                "get-resources" => s.sync_layout_p99_ms = None,
                _ => {}
            }
        }
        let v = violations(&r, &thresholds());
        assert_eq!(
            v,
            [
                "atlas: synchronous work p99 3.00 ms a frame (max 2)",
                "get-resources: no synchronous work measured",
            ],
        );
    }
}
