# GPUI prototype vs egui app — prototype/gpui-text-paint/bench-out/xvfb-smoke

Runs: gpui 1, egui 1; display 60.00 Hz; medians over runs. **egui/gpui** > 1 means GPUI is lower (better).

- egui: window 1400×900 points at scale 1.0; stalls over 1 s (window hidden), left out of the statistics: 0

| Measure | GPUI | egui | egui/gpui |
|---|---:|---:|---:|
| Startup: framework ready, before any app data (ms) | 7.14 | 228 | 31.97× |
| Startup: four Bibles read (ms) | 2188 | 4853 | 2.22× |
| Startup: window's first frame begun (ms) | 76.87 | 4859 | 63.21× |
| Startup: first frame painted (ms) | 82.15 | 5101 | 62.10× |
| Startup: all Panels showing text (ms) | 2195 | 5153 | 2.35× |
| Peak RSS (MB) | 850 | 841 | 0.99× |
| Process CPU per frame, all threads (ms) | 48.49 | 35.40 | 0.73× |
| Scenario wall time (ms) | 81856 | 35398 | 0.43× |

## Frame interval (frame start to next frame start: what you see)

| Phase | Stat | GPUI | egui | egui/gpui |
|---|---|---:|---:|---:|
| all | p50_ms | 48.00 | 19.17 | 0.40× |
| all | p95_ms | 48.04 | 41.69 | 0.87× |
| all | p99_ms | 48.09 | 51.56 | 1.07× |
| all | max_ms | 48.58 | 70.14 | 1.44× |
| all | missed_pct | 100 | 24.79 | 0.25× |
| scroll | p50_ms | 48.00 | 13.05 | 0.27× |
| scroll | p95_ms | 48.04 | 43.21 | 0.90× |
| scroll | p99_ms | 48.09 | 51.56 | 1.07× |
| scroll | max_ms | 48.58 | 62.73 | 1.29× |
| scroll | missed_pct | 100 | 17.22 | 0.17× |
| fling | p50_ms | 48.00 | 22.29 | 0.46× |
| fling | p95_ms | 48.03 | 39.19 | 0.82× |
| fling | p99_ms | 48.05 | 49.49 | 1.03× |
| fling | max_ms | 48.12 | 56.53 | 1.17× |
| fling | missed_pct | 100 | 32.01 | 0.32× |
| jump | p50_ms | 48.00 | 24.85 | 0.52× |
| jump | p95_ms | 48.03 | 35.26 | 0.73× |
| jump | p99_ms | 48.07 | 50.98 | 1.06× |
| jump | max_ms | 48.08 | 70.14 | 1.46× |
| jump | missed_pct | 100 | 47.92 | 0.48× |
| idle | p50_ms | 48.00 | 6.19 | 0.13× |
| idle | p95_ms | 48.07 | 40.85 | 0.85× |
| idle | p99_ms | 48.09 | 54.24 | 1.13× |
| idle | max_ms | 48.09 | 58.39 | 1.21× |
| idle | missed_pct | 100 | 14.44 | 0.14× |

## UI-thread frame work (frame start to end of the app's paint/UI pass)

| Phase | Stat | GPUI | egui | egui/gpui |
|---|---|---:|---:|---:|
| all | p50_ms | 2.23 | 1.61 | 0.72× |
| all | p95_ms | 2.95 | 2.52 | 0.86× |
| all | p99_ms | 3.40 | 4.93 | 1.45× |
| all | max_ms | 16.80 | 48.35 | 2.88× |
| all | missed_pct | 0.00 | 0.07 |  |
| scroll | p50_ms | 2.17 | 1.47 | 0.68× |
| scroll | p95_ms | 2.87 | 1.86 | 0.65× |
| scroll | p99_ms | 3.16 | 3.35 | 1.06× |
| scroll | max_ms | 3.75 | 5.58 | 1.49× |
| scroll | missed_pct | 0.00 | 0.00 |  |
| fling | p50_ms | 2.55 | 1.74 | 0.68× |
| fling | p95_ms | 3.10 | 2.10 | 0.68× |
| fling | p99_ms | 3.29 | 3.12 | 0.95× |
| fling | max_ms | 3.73 | 5.70 | 1.53× |
| fling | missed_pct | 0.00 | 0.00 |  |
| jump | p50_ms | 2.37 | 2.06 | 0.87× |
| jump | p95_ms | 3.15 | 3.18 | 1.01× |
| jump | p99_ms | 9.14 | 8.69 | 0.95× |
| jump | max_ms | 16.80 | 48.35 | 2.88× |
| jump | missed_pct | 0.00 | 0.42 |  |
| idle | p50_ms | 2.14 | 1.28 | 0.60× |
| idle | p95_ms | 2.76 | 1.75 | 0.63× |
| idle | p99_ms | 2.83 | 2.33 | 0.82× |
| idle | max_ms | 3.56 | 4.99 | 1.40× |
| idle | missed_pct | 0.00 | 0.00 |  |

## egui only: eframe's frame CPU (UI + tessellation + GL calls, not the swap)

| Phase | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| all | 4.53 | 20.13 | 27.94 | 54.15 |
| scroll | 3.96 | 22.17 | 28.01 | 35.18 |
| fling | 6.51 | 16.90 | 25.78 | 29.05 |
| jump | 6.81 | 12.15 | 29.14 | 54.15 |
| idle | 3.06 | 21.60 | 25.97 | 26.75 |

## GPUI only: our text pipeline on GPUI (#372)

| Measure | median over runs |
|---|---:|
| Frames that showed a block not yet laid out | 0 |
| Blocks shown blank (total) | 0 |
| UI-thread layout per frame, p50 (ms) | 0.00 |
| UI-thread layout per frame, p99 (ms) | 0.00 |
| UI-thread layout per frame, max (ms) | 14.52 |
| Blocks laid out on the UI thread | 96 |
| Blocks laid out by workers | 376 |
| Glyphs painted per frame, p50 | 2765 |
| Shape cache hit rate | 0.85 |

missed_pct: frames whose interval/work exceeded 1.5× the display's frame budget.
