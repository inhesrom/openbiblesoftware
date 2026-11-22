# GPUI prototype vs egui app — prototype/gpui-shell/bench-out/20261121-111840-omarchy

Runs: gpui 3, egui 3; display 120 Hz; medians over runs. **egui/gpui** > 1 means GPUI is lower (better).

| Measure | GPUI | egui | egui/gpui |
|---|---:|---:|---:|
| Startup: framework ready, before any app data (ms) | 23.41 | 30.19 | 1.29× |
| Startup: four Bibles read (ms) | 469 | 2882 | 6.15× |
| Startup: window's first frame begun (ms) | 46.33 | 2882 | 62.21× |
| Startup: first frame painted (ms) | 49.06 | 2891 | 58.93× |
| Startup: all Panels showing text (ms) | 561 | 2899 | 5.17× |
| Peak RSS (MB) | 775 | 795 | 1.03× |
| Process CPU per frame, all threads (ms) | 2.03 | 7.58 | 3.74× |
| Scenario wall time (ms) | 18075 | 20475 | 1.13× |

## Frame interval (frame start to next frame start: what you see)

| Phase | Stat | GPUI | egui | egui/gpui |
|---|---|---:|---:|---:|
| all | p50_ms | 8.33 | 8.36 | 1.00× |
| all | p95_ms | 8.50 | 16.79 | 1.98× |
| all | p99_ms | 16.89 | 24.93 | 1.48× |
| all | max_ms | 125 | 33.59 | 0.27× |
| all | missed_pct | 2.94 | 25.51 | 8.69× |
| scroll | p50_ms | 8.33 | 8.38 | 1.01× |
| scroll | p95_ms | 8.47 | 16.81 | 1.98× |
| scroll | p99_ms | 16.80 | 24.93 | 1.48× |
| scroll | max_ms | 50.15 | 33.59 | 0.67× |
| scroll | missed_pct | 3.00 | 30.00 | 10.00× |
| fling | p50_ms | 8.33 | 8.35 | 1.00× |
| fling | p95_ms | 8.47 | 16.76 | 1.98× |
| fling | p99_ms | 16.77 | 24.98 | 1.49× |
| fling | max_ms | 16.81 | 25.09 | 1.49× |
| fling | missed_pct | 3.30 | 18.15 | 5.50× |
| jump | p50_ms | 8.33 | 8.36 | 1.00× |
| jump | p95_ms | 8.63 | 16.75 | 1.94× |
| jump | p99_ms | 100 | 16.96 | 0.17× |
| jump | max_ms | 125 | 25.02 | 0.20× |
| jump | missed_pct | 3.33 | 26.67 | 8.00× |
| idle | p50_ms | 8.33 | 8.33 | 1.00× |
| idle | p95_ms | 8.46 | 8.43 | 1.00× |
| idle | p99_ms | 8.51 | 8.51 | 1.00× |
| idle | max_ms | 8.59 | 16.62 | 1.94× |
| idle | missed_pct | 0.00 | 1.11 |  |

## UI-thread frame work (frame start to end of the app's paint/UI pass)

| Phase | Stat | GPUI | egui | egui/gpui |
|---|---|---:|---:|---:|
| all | p50_ms | 1.09 | 2.35 | 2.16× |
| all | p95_ms | 4.10 | 4.16 | 1.01× |
| all | p99_ms | 12.20 | 6.40 | 0.52× |
| all | max_ms | 115 | 16.12 | 0.14× |
| all | missed_pct | 0.98 | 0.13 | 0.13× |
| scroll | p50_ms | 1.11 | 2.37 | 2.14× |
| scroll | p95_ms | 4.13 | 3.66 | 0.89× |
| scroll | p99_ms | 10.63 | 4.16 | 0.39× |
| scroll | max_ms | 42.07 | 13.57 | 0.32× |
| scroll | missed_pct | 0.67 | 0.11 | 0.17× |
| fling | p50_ms | 1.08 | 2.27 | 2.11× |
| fling | p95_ms | 4.75 | 3.39 | 0.71× |
| fling | p99_ms | 9.51 | 3.76 | 0.40× |
| fling | max_ms | 12.49 | 4.67 | 0.37× |
| fling | missed_pct | 0.00 | 0.00 |  |
| jump | p50_ms | 1.00 | 2.58 | 2.59× |
| jump | p95_ms | 1.68 | 6.50 | 3.86× |
| jump | p99_ms | 90.63 | 7.17 | 0.08× |
| jump | max_ms | 115 | 16.12 | 0.14× |
| jump | missed_pct | 3.33 | 0.42 | 0.12× |
| idle | p50_ms | 1.04 | 1.84 | 1.78× |
| idle | p95_ms | 1.18 | 2.82 | 2.38× |
| idle | p99_ms | 1.23 | 3.12 | 2.54× |
| idle | max_ms | 1.27 | 3.29 | 2.59× |
| idle | missed_pct | 0.00 | 0.00 |  |

## egui only: eframe's frame CPU (UI + tessellation + GL calls, not the swap)

| Phase | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| all | 4.38 | 10.91 | 15.46 | 24.67 |
| scroll | 4.86 | 10.83 | 16.04 | 24.67 |
| fling | 4.18 | 12.81 | 15.46 | 17.47 |
| jump | 3.45 | 9.86 | 13.38 | 19.61 |
| idle | 2.56 | 3.74 | 4.23 | 10.78 |

missed_pct: frames whose interval/work exceeded 1.5× the display's frame budget.
