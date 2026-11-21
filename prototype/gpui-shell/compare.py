#!/usr/bin/env python3
"""PROTOTYPE — summarize bench.sh's JSON reports (gpui-N.json, egui-N.json) side by side.

    python3 prototype/gpui-shell/compare.py <bench-out folder>

Each figure is the median over that app's runs. Lower is better everywhere.
"""
import json
import statistics
import sys
from pathlib import Path


def load(folder, app):
    return [json.loads(p.read_text()) for p in sorted(Path(folder).glob(f"{app}-*.json"))]


def get(d, *path):
    for k in path:
        if not isinstance(d, dict) or k not in d:
            return None
        d = d[k]
    return d


def med(runs, *path):
    vals = [v for v in (get(r, *path) for r in runs) if isinstance(v, (int, float))]
    return statistics.median(vals) if vals else None


def fmt(v, unit=""):
    if v is None:
        return "–"
    if isinstance(v, float):
        v = f"{v:.2f}" if abs(v) < 100 else f"{v:.0f}"
    return f"{v}{unit}"


def ratio(g, e):
    if g is None or e is None or g == 0:
        return ""
    return f"{e / g:.2f}×"


def main():
    folder = sys.argv[1] if len(sys.argv) > 1 else "."
    runs = {app: load(folder, app) for app in ("gpui", "egui")}
    if not runs["gpui"] or not runs["egui"]:
        print(f"need gpui-*.json and egui-*.json in {folder}")
        return 1
    hz = med(runs["gpui"], "display_hz")
    print(f"# GPUI prototype vs egui app — {folder}\n")
    print(f"Runs: gpui {len(runs['gpui'])}, egui {len(runs['egui'])}; display {fmt(hz)} Hz; "
          "medians over runs. **egui/gpui** > 1 means GPUI is lower (better).\n")
    rows = [
        ("Startup: framework ready, before any app data (ms)", ("startup", "framework_ready_ms")),
        ("Startup: four Bibles read (ms)", ("startup", "data_loaded_ms")),
        ("Startup: window's first frame begun (ms)", ("startup", "window_ms")),
        ("Startup: first frame painted (ms)", ("startup", "first_frame_ms")),
        ("Startup: all Panels showing text (ms)", ("startup", "ready_ms")),
        ("Peak RSS (MB)", ("peak_rss_mb",)),
        ("Process CPU per frame, all threads (ms)", ("cpu", "process_cpu_ms_per_frame")),
        ("Scenario wall time (ms)", ("cpu", "scenario_wall_ms")),
    ]
    print("| Measure | GPUI | egui | egui/gpui |\n|---|---:|---:|---:|")
    for label, path in rows:
        g, e = med(runs["gpui"], *path), med(runs["egui"], *path)
        print(f"| {label} | {fmt(g)} | {fmt(e)} | {ratio(g, e)} |")
    phases = ["all", "scroll", "fling", "jump", "idle"]
    for kind, title in (("frame_interval", "Frame interval (frame start to next frame start: what you see)"),
                        ("frame_work", "UI-thread frame work (frame start to end of the app's paint/UI pass)")):
        print(f"\n## {title}\n")
        print("| Phase | Stat | GPUI | egui | egui/gpui |\n|---|---|---:|---:|---:|")
        for ph in phases:
            for stat in ("p50_ms", "p95_ms", "p99_ms", "max_ms", "missed_pct"):
                g = med(runs["gpui"], kind, ph, stat)
                e = med(runs["egui"], kind, ph, stat)
                if g is None and e is None:
                    continue
                print(f"| {ph} | {stat} | {fmt(g)} | {fmt(e)} | {ratio(g, e)} |")
    if any(get(r, "eframe_frame_cpu") for r in runs["egui"]):
        print("\n## egui only: eframe's frame CPU (UI + tessellation + GL calls, not the swap)\n")
        print("| Phase | p50 | p95 | p99 | max |\n|---|---:|---:|---:|---:|")
        for ph in phases:
            vals = [fmt(med(runs["egui"], "eframe_frame_cpu", ph, st)) for st in ("p50_ms", "p95_ms", "p99_ms", "max_ms")]
            print(f"| {ph} | " + " | ".join(vals) + " |")
    print("\nmissed_pct: frames whose interval/work exceeded 1.5× the display's frame budget.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
