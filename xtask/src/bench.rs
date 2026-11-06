//! `cargo xtask bench`: run the headless reading-pane benchmark in release mode and fail if
//! any number is past its threshold in `xtask/bench.toml` (#19).

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
        eprintln!(
            "  {:<7} missed {:>5.2}%  placeholder frames {:>3}  frame CPU p99 {:>5.2} ms",
            s.name, s.missed_pct, s.placeholder_frames, s.cpu_p99_ms
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

    fn report(steady_missed: f64, placeholders: usize, sync_p99: f64) -> Report {
        Report {
            scenarios: vec![
                Scenario {
                    name: "steady".into(),
                    missed_pct: steady_missed,
                    placeholder_frames: placeholders,
                    cpu_p99_ms: 2.0,
                },
                Scenario {
                    name: "fling".into(),
                    missed_pct: 0.0,
                    placeholder_frames: 0,
                    cpu_p99_ms: 2.0,
                },
                Scenario {
                    name: "jumps".into(),
                    missed_pct: 2.0,
                    placeholder_frames: 20,
                    cpu_p99_ms: 8.0,
                },
            ],
            sync_layout_p99_ms: sync_p99,
            jump_origin_settle_frames_max: 1,
            jump_all_settle_frames_max: 2,
            text_gpu_mib: 4.0,
            find_matches: 0,
            find_ms: 0.0,
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
}
