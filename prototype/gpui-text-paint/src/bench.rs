//! PROTOTYPE — throwaway. The scripted scenario (`scenario.json`) and the frame report, shared
//! word for word by both apps: the egui app compiles this same file into
//! `crates/ui-egui/src/proto_bench.rs` (`#[path]`), so the two JSON reports compare like for like.
#![allow(dead_code)] // Each app uses part of it.

use std::time::Instant;

use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Scroll(f32),
    Jump(String),
    /// Hold still here; the window is ready to be captured as `<app>-<name>.png`.
    Shot(String),
    /// Repeat this frame (doing nothing) until this many seconds have passed since it was
    /// first reached: lets time-based effects (the egui app's 1.2 s jump flash) end before a
    /// screenshot, whatever the frame rate. Only in `shot` steps, which are not timed.
    Settle(f32),
}

/// Runs a scenario's frames in order, holding on `Settle` until its time has passed.
#[derive(Debug, Default)]
pub struct Cursor {
    next: usize,
    settle_since: Option<Instant>,
}

impl Cursor {
    /// This frame's phase and action; None once the scenario is over.
    pub fn step(&mut self, scenario: &Scenario, now: Instant) -> Option<(String, Action)> {
        let (phase, action) = scenario.frames.get(self.next)?.clone();
        if let Action::Settle(secs) = action {
            let since = *self.settle_since.get_or_insert(now);
            if now.duration_since(since).as_secs_f32() < secs {
                return Some((phase, Action::None));
            }
            self.settle_since = None;
        }
        self.next += 1;
        Some((phase, action))
    }
}

#[derive(Debug, Clone)]
pub struct Scenario {
    pub window: [f32; 2],
    pub bibles: Vec<String>,
    /// One entry per frame: (phase, action).
    pub frames: Vec<(String, Action)>,
}

pub fn load(path: &str) -> Result<Scenario, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let window = [
        v["window"][0].as_f64().unwrap_or(1400.0) as f32,
        v["window"][1].as_f64().unwrap_or(900.0) as f32,
    ];
    let bibles = v["bibles"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|b| b.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let mut frames = Vec::new();
    for step in v["steps"].as_array().cloned().unwrap_or_default() {
        let phase = step["phase"].as_str().unwrap_or("?").to_string();
        let n = |k: &str, d: f64| step[k].as_f64().unwrap_or(d);
        let mut push = |a: Action| frames.push((phase.clone(), a));
        if let Some(name) = step["shot"].as_str() {
            // Jump, settle, mark the shot, then hold still for the capture.
            if let Some(to) = step["jump"].as_str() {
                push(Action::Jump(to.to_string()));
            }
            push(Action::Settle(n("settle_secs", 1.5) as f32));
            for _ in 0..n("settle", 10.0) as usize {
                push(Action::None);
            }
            push(Action::Shot(name.to_string()));
            for _ in 0..n("hold", 90.0) as usize {
                push(Action::None);
            }
        } else if let Some(to) = step["jump"].as_str() {
            push(Action::Jump(to.to_string()));
            for _ in 1..n("settle", 30.0) as usize {
                push(Action::None);
            }
        } else if let Some(list) = step["to"].as_array() {
            for to in list.iter().filter_map(Value::as_str) {
                push(Action::Jump(to.to_string()));
                for _ in 1..n("settle", 30.0) as usize {
                    push(Action::None);
                }
            }
        } else if step.get("v0").is_some() {
            for _ in 0..n("repeat", 1.0) as usize {
                let mut v = n("v0", 100.0) as f32;
                let decay = n("decay", 0.95) as f32;
                while v >= 1.0 {
                    push(Action::Scroll(v));
                    v *= decay;
                }
                for _ in 0..n("rest", 0.0) as usize {
                    push(Action::None);
                }
            }
        } else {
            let dy = n("dy", 0.0) as f32;
            for _ in 0..n("frames", 0.0) as usize {
                push(if dy == 0.0 {
                    Action::None
                } else {
                    Action::Scroll(dy)
                });
            }
        }
    }
    Ok(Scenario {
        window,
        bibles,
        frames,
    })
}

/// Per-frame wall-clock record: when each frame's UI started and when its paint ended; and
/// the process's CPU time (all threads: UI, workers, GPU driver) from the first scenario frame
/// to the end.
#[derive(Debug, Default)]
pub struct Recorder {
    pub starts: Vec<Instant>,
    pub paint_ends: Vec<Option<Instant>>,
    pub phases: Vec<String>,
    /// egui only: eframe's own CPU time of each frame (UI, tessellation, GL calls; not the
    /// buffer swap). GPUI has no equivalent hook.
    pub eframe_cpu_ms: Vec<Option<f64>>,
    cpu_start: Option<(f64, Instant)>,
    cpu_end: Option<(f64, Instant)>,
}

/// CPU time of the whole process so far, in ms (Linux `/proc`; 0 elsewhere).
pub fn process_cpu_ms() -> f64 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| {
            // Fields after the parenthesised command name; utime and stime are 14th and 15th.
            let rest = s.rsplit_once(')')?.1;
            let f: Vec<&str> = rest.split_whitespace().collect();
            let ticks = f.get(11)?.parse::<f64>().ok()? + f.get(12)?.parse::<f64>().ok()?;
            Some(ticks * 10.0) // USER_HZ is 100 on Linux.
        })
        .unwrap_or(0.0)
}

impl Recorder {
    pub fn frame_start(&mut self, at: Instant, phase: &str) {
        if phase != "setup" && self.cpu_start.is_none() {
            self.cpu_start = Some((process_cpu_ms(), at));
        }
        if phase == "done" {
            self.cpu_end = Some((process_cpu_ms(), at));
        }
        self.starts.push(at);
        self.paint_ends.push(None);
        self.phases.push(phase.to_string());
        self.eframe_cpu_ms.push(None);
    }
    /// eframe reports a frame's CPU time during the next one.
    pub fn previous_frame_cpu(&mut self, ms: f64) {
        let n = self.eframe_cpu_ms.len();
        if n >= 2 {
            self.eframe_cpu_ms[n - 2] = Some(ms);
        }
    }
    pub fn paint_end(&mut self, at: Instant) {
        if let Some(last) = self.paint_ends.last_mut() {
            last.get_or_insert(at);
        }
    }
}

/// A frame interval longer than this is not the app's doing: the compositor stopped asking
/// for frames (the window was hidden, e.g. on another workspace). Counted apart as a stall.
const STALL_MS: f64 = 1000.0;

fn stats(v: &mut Vec<f64>, hz: f64) -> Value {
    let stalls = v.iter().filter(|x| **x > STALL_MS).count();
    v.retain(|x| *x <= STALL_MS);
    if v.is_empty() {
        return json!({ "count": 0, "stalls_over_1s": stalls });
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    let budget = 1000.0 / hz;
    let missed = v.iter().filter(|x| **x > budget * 1.5).count();
    json!({
        "count": v.len(),
        "mean_ms": v.iter().sum::<f64>() / v.len() as f64,
        "p50_ms": q(0.50),
        "p95_ms": q(0.95),
        "p99_ms": q(0.99),
        "max_ms": v[v.len() - 1],
        "missed_frames": missed,
        "missed_pct": 100.0 * missed as f64 / v.len() as f64,
        "stalls_over_1s": stalls,
    })
}

/// `OBS_PROTO_SHOT_DIR`: say the window is ready to capture as `<app>-<name>.png` and wait
/// (at most 10 s) for the bench script's watcher to say it has taken it (`.taken`), so both
/// apps are captured alike, from outside, on a settled frame. Shot frames are not timed.
pub fn mark_shot(app: &str, name: &str) {
    let Some(dir) = std::env::var_os("OBS_PROTO_SHOT_DIR") else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    let ready = dir.join(format!("{app}-{name}.ready"));
    if let Err(e) = std::fs::write(&ready, b"") {
        eprintln!("bench: {}: {e}", ready.display());
        return;
    }
    let taken = dir.join(format!("{app}-{name}.taken"));
    let until = Instant::now() + std::time::Duration::from_secs(10);
    while !taken.exists() && Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

pub fn peak_rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
        })
        .map_or(0.0, |kb| kb / 1024.0)
}

/// The report: frame intervals (start to next start, what the user sees) and frame work
/// (UI start to paint end, CPU only) by phase. `setup` and `shot` frames (loading, and holding
/// still while a screenshot is taken) are left out of `all`.
pub fn report(rec: &Recorder, hz: f64, extra: Value) -> Value {
    let mut by: std::collections::BTreeMap<String, (Vec<f64>, Vec<f64>, Vec<f64>)> =
        Default::default();
    for k in 0..rec.starts.len() {
        let phase = rec.phases[k].clone();
        // The frame after the last only closes the last interval.
        if phase == "done" {
            continue;
        }
        let interval = rec
            .starts
            .get(k + 1)
            .map(|n| n.duration_since(rec.starts[k]).as_secs_f64() * 1000.0);
        let work =
            rec.paint_ends[k].map(|e| e.duration_since(rec.starts[k]).as_secs_f64() * 1000.0);
        for key in [phase.clone(), "all".to_string()] {
            if key == "all" && (phase == "setup" || phase == "shot") {
                continue;
            }
            let e = by.entry(key).or_default();
            if let Some(i) = interval {
                e.0.push(i);
            }
            if let Some(w) = work {
                e.1.push(w);
            }
            if let Some(c) = rec.eframe_cpu_ms.get(k).copied().flatten() {
                e.2.push(c);
            }
        }
    }
    let mut intervals = serde_json::Map::new();
    let mut work = serde_json::Map::new();
    let mut eframe = serde_json::Map::new();
    for (k, (mut i, mut w, mut c)) in by {
        intervals.insert(k.clone(), stats(&mut i, hz));
        work.insert(k.clone(), stats(&mut w, hz));
        if !c.is_empty() {
            eframe.insert(k, stats(&mut c, hz));
        }
    }
    // Every timed frame, so the statistics can be recomputed (another display rate, say).
    let mut raw_phase = Vec::new();
    let mut raw_interval = Vec::new();
    let mut raw_work = Vec::new();
    for k in 0..rec.starts.len() {
        if matches!(rec.phases[k].as_str(), "setup" | "done") {
            continue;
        }
        raw_phase.push(rec.phases[k].clone());
        raw_interval.push(
            rec.starts
                .get(k + 1)
                .map(|n| n.duration_since(rec.starts[k]).as_secs_f64() * 1000.0),
        );
        raw_work.push(
            rec.paint_ends[k].map(|e| e.duration_since(rec.starts[k]).as_secs_f64() * 1000.0),
        );
    }
    let timed = by_count(rec);
    let cpu = match (rec.cpu_start, rec.cpu_end) {
        (Some((c0, t0)), Some((c1, t1))) => json!({
            "scenario_wall_ms": t1.duration_since(t0).as_secs_f64() * 1000.0,
            "process_cpu_ms": c1 - c0,
            "process_cpu_ms_per_frame": (c1 - c0) / timed.max(1) as f64,
            "frames": timed,
        }),
        _ => json!(null),
    };
    let mut out = json!({
        "display_hz": hz,
        "frame_interval": intervals,
        "frame_work": work,
        "eframe_frame_cpu": if eframe.is_empty() { json!(null) } else { Value::Object(eframe) },
        "cpu": cpu,
        "peak_rss_mb": peak_rss_mb(),
        "raw": { "phase": raw_phase, "interval_ms": raw_interval, "work_ms": raw_work },
    });
    if let (Some(o), Some(e)) = (out.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            o.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Scenario frames recorded (everything after setup, before done).
fn by_count(rec: &Recorder) -> usize {
    rec.phases
        .iter()
        .filter(|p| *p != "setup" && *p != "done")
        .count()
}
