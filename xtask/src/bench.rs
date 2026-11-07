//! `cargo xtask bench`: run the headless reading-pane benchmark and its feature scenarios
//! (#183) in release mode and fail if any number is past its threshold in `xtask/bench.toml`
//! (#19), or a feature scenario listed there did not run.

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
        if let Some(max) = limits.max_placeholder_frames {
            check(
                s.placeholder_frames <= max,
                format!(
                    "{name}: {} placeholder frames (max {max})",
                    s.placeholder_frames
                ),
            );
        }
    }
    out
}

pub fn run(root: &Path) -> Result<()> {
    let thresholds: Thresholds = toml::from_str(
        &std::fs::read_to_string(root.join("xtask/bench.toml"))
            .context("cannot read xtask/bench.toml")?,
    )
    .context("cannot parse xtask/bench.toml")?;
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
        eprintln!(
            "  {:<16} missed {:>5.2}%  placeholder frames {:>3}  frame CPU p50 {:>5.2} ms, p99 {:>5.2} ms{sync}{settle}",
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
            "atlas",
            "search-grid",
            "search-chart",
            "exegetical-guide",
            "topic-guide",
            "find",
            "get-resources",
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
