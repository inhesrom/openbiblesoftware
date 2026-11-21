# GPUI prototype vs egui app — bench-out/xvfb-llvmpipe

Runs: gpui 3, egui 3; display 60.00 Hz; medians over runs. **egui/gpui** > 1 means GPUI is lower (better).

| Measure | GPUI | egui | egui/gpui |
|---|---:|---:|---:|
| Startup: framework ready, before any app data (ms) | 35.30 | 39.60 | 1.12× |
| Startup: four Bibles read (ms) | 878 | 3807 | 4.33× |
| Startup: window's first frame begun (ms) | 62.45 | 3807 | 60.96× |
| Startup: first frame painted (ms) | 64.77 | 3817 | 58.93× |
| Startup: all Panels showing text (ms) | 971 | 3843 | 3.96× |
| Peak RSS (MB) | 877 | 844 | 0.96× |
| Process CPU per frame, all threads (ms) | 45.19 | 36.21 | 0.80× |
| Scenario wall time (ms) | 29712 | 11463 | 0.39× |

## Frame interval (frame start to next frame start: what you see)

| Phase | Stat | GPUI | egui | egui/gpui |
|---|---|---:|---:|---:|
| all | p50_ms | 16.00 | 4.94 | 0.31× |
| all | p95_ms | 16.47 | 5.62 | 0.34× |
| all | p99_ms | 16.88 | 6.17 | 0.37× |
| all | max_ms | 112 | 16.66 | 0.15× |
| all | missed_pct | 0.72 | 0.00 | 0.00× |
| scroll | p50_ms | 16.00 | 4.88 | 0.30× |
| scroll | p95_ms | 16.45 | 5.35 | 0.33× |
| scroll | p99_ms | 16.73 | 5.60 | 0.33× |
| scroll | max_ms | 48.28 | 9.47 | 0.20× |
| scroll | missed_pct | 0.33 | 0.00 | 0.00× |
| fling | p50_ms | 16.00 | 4.89 | 0.31× |
| fling | p95_ms | 16.49 | 5.34 | 0.32× |
| fling | p99_ms | 16.69 | 5.53 | 0.33× |
| fling | max_ms | 16.86 | 6.20 | 0.37× |
| fling | missed_pct | 0.00 | 0.00 |  |
| jump | p50_ms | 16.00 | 5.45 | 0.34× |
| jump | p95_ms | 16.63 | 6.17 | 0.37× |
| jump | p99_ms | 80.19 | 7.85 | 0.10× |
| jump | max_ms | 112 | 16.66 | 0.15× |
| jump | missed_pct | 3.33 | 0.00 | 0.00× |
| idle | p50_ms | 16.00 | 4.80 | 0.30× |
| idle | p95_ms | 16.44 | 5.35 | 0.33× |
| idle | p99_ms | 16.60 | 5.48 | 0.33× |
| idle | max_ms | 16.68 | 5.61 | 0.34× |
| idle | missed_pct | 0.00 | 0.00 |  |

## UI-thread frame work (frame start to end of the app's paint/UI pass)

| Phase | Stat | GPUI | egui | egui/gpui |
|---|---|---:|---:|---:|
| all | p50_ms | 1.32 | 0.94 | 0.71× |
| all | p95_ms | 3.20 | 1.37 | 0.43× |
| all | p99_ms | 8.67 | 1.71 | 0.20× |
| all | max_ms | 99.11 | 12.35 | 0.12× |
| all | missed_pct | 0.59 | 0.00 | 0.00× |
| scroll | p50_ms | 1.32 | 0.92 | 0.70× |
| scroll | p95_ms | 2.98 | 1.14 | 0.38× |
| scroll | p99_ms | 8.18 | 1.26 | 0.15× |
| scroll | max_ms | 28.08 | 3.23 | 0.11× |
| scroll | missed_pct | 0.11 | 0.00 | 0.00× |
| fling | p50_ms | 1.30 | 0.93 | 0.72× |
| fling | p95_ms | 3.97 | 1.13 | 0.29× |
| fling | p99_ms | 6.15 | 1.23 | 0.20× |
| fling | max_ms | 9.24 | 1.28 | 0.14× |
| fling | missed_pct | 0.00 | 0.00 |  |
| jump | p50_ms | 1.37 | 1.18 | 0.86× |
| jump | p95_ms | 2.01 | 1.76 | 0.88× |
| jump | p99_ms | 69.98 | 3.53 | 0.05× |
| jump | max_ms | 99.11 | 12.35 | 0.12× |
| jump | missed_pct | 3.33 | 0.00 | 0.00× |
| idle | p50_ms | 1.29 | 0.74 | 0.57× |
| idle | p95_ms | 1.72 | 1.14 | 0.66× |
| idle | p99_ms | 1.80 | 1.18 | 0.66× |
| idle | max_ms | 1.81 | 1.20 | 0.66× |
| idle | missed_pct | 0.00 | 0.00 |  |

## egui only: eframe's frame CPU (UI + tessellation + GL calls, not the swap)

| Phase | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| all | 2.39 | 2.88 | 3.34 | 13.89 |
| scroll | 2.36 | 2.66 | 2.89 | 6.74 |
| fling | 2.37 | 2.67 | 2.83 | 2.93 |
| jump | 2.72 | 3.34 | 5.26 | 13.89 |
| idle | 2.19 | 2.63 | 2.77 | 2.84 |

missed_pct: frames whose interval/work exceeded 1.5× the display's frame budget.
