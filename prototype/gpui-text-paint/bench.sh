#!/usr/bin/env bash
# PROTOTYPE — throwaway (#372, branch prototype/372-gpui-text-paint). Builds the GPUI prototype
# that paints our own laid-out text (prototype/gpui-text-paint)
# and the egui app in release, runs the same windowed scenario (scenario.json) in each, and
# writes their JSON reports, screenshots and a side-by-side summary to one folder.
#
#   prototype/gpui-text-paint/bench.sh                # 1 run of each
#   RUNS=3 prototype/gpui-text-paint/bench.sh         # alternate gpui, egui, gpui, egui, …
#
# Env: OBS_LIBRARY (default ~/.local/share/OpenBible/library, read only), OBS_PROTO_HZ
# (display refresh for the missed-frame count; X11: read from xrandr, else 60), OUT (folder),
# SCENARIO, SKIP_BUILD=1, NO_SHOTS=1. Nothing is written to your library or User database:
# both apps get a scratch XDG_DATA_HOME / XDG_CONFIG_HOME.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
SCENARIO=${SCENARIO:-$HERE/scenario.json}
RUNS=${RUNS:-1}
LIB=${OBS_LIBRARY:-${XDG_DATA_HOME:-$HOME/.local/share}/OpenBible/library}
OUT=${OUT:-$HERE/bench-out/$(date +%Y%m%d-%H%M%S)-$(hostname -s 2>/dev/null || echo host)}
mkdir -p "$OUT/shots"
OUT=$(cd "$OUT" && pwd)

for id in kjv-strongs web tahot tagnt; do
  [ -f "$LIB/$id.obr" ] || { echo "missing $LIB/$id.obr (set OBS_LIBRARY)"; exit 1; }
done

# The display's refresh rate, for the missed-frame count: OBS_PROTO_HZ, else asked of
# Hyprland, wlroots compositors (wlr-randr) or X11 (xrandr), else 60.
if [ -z "${OBS_PROTO_HZ:-}" ]; then
  hz=
  if [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] && command -v hyprctl >/dev/null; then
    hz=$(hyprctl monitors -j 2>/dev/null | python3 -c 'import json, sys
m = json.load(sys.stdin)
m = [x for x in m if x.get("focused")] or m
print(round(m[0]["refreshRate"]) if m else "")' 2>/dev/null)
  elif [ -n "${WAYLAND_DISPLAY:-}" ] && command -v wlr-randr >/dev/null; then
    hz=$(wlr-randr 2>/dev/null | awk '/current/ { for (i = 1; i <= NF; i++) if ($i == "Hz") { printf "%.0f\n", $(i-1); exit } }')
  elif [ -n "${DISPLAY:-}" ] && command -v xrandr >/dev/null; then
    hz=$(xrandr --current 2>/dev/null | awk '/\*/ { for (i = 2; i <= NF; i++) if ($i ~ /\*/) { gsub(/[*+]/, "", $i); print $i; exit } }')
  fi
  if [ -n "$hz" ] && awk "BEGIN { exit !($hz > 1) }"; then
    OBS_PROTO_HZ=$hz
  else
    OBS_PROTO_HZ=60
    echo "warning: could not read the display's refresh rate; assuming 60 Hz (set OBS_PROTO_HZ)"
  fi
fi
export OBS_PROTO_HZ
echo "Display refresh: $OBS_PROTO_HZ Hz. Keep the benchmark windows visible until it finishes:"
echo "a hidden window (another workspace, minimized) gets no frames and stalls the run."


GPUI_BIN=$HERE/target/release/obs-prototype-gpui-text-paint
EGUI_BIN=${EGUI_BIN:-$ROOT/target/release/openbible}
if [ -z "${SKIP_BUILD:-}" ]; then
  echo "== building the GPUI prototype (release)"
  # From the crate folder, so its .cargo/config.toml (fontconfig via dlopen) applies.
  (cd "$HERE" && cargo build --release)
  echo "== building the egui app (release)"
  (cd "$ROOT" && cargo build --release -p openbible)
fi

# What was measured on.
{
  echo "date: $(date -Iseconds)"
  echo "host: $(uname -srm)"
  echo "cpu: $(awk -F': ' '/model name/ { print $2; exit }' /proc/cpuinfo 2>/dev/null)"
  echo "gpu: $(lspci 2>/dev/null | grep -iE 'vga|3d|display' | sed 's/^[^ ]* //' | paste -sd ';')"
  echo "session: ${XDG_SESSION_TYPE:-?} DISPLAY=${DISPLAY:-} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}"
  echo "display_hz: $OBS_PROTO_HZ"
  echo "gpui binary: $(du -h "$GPUI_BIN" | cut -f1)"
  echo "egui binary: $(du -h "$EGUI_BIN" | cut -f1)"
  echo "scenario: $SCENARIO"
} > "$OUT/system.txt"
cat "$OUT/system.txt"

SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/obs-proto-bench.XXXXXX")
trap 'rm -rf "$SCRATCH"' EXIT

# Screenshot of the window titled $1 into $2: X11 (import), or wlroots Wayland (grim, whole screen).
capture() {
  local title=$1 png=$2
  if [ -n "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ] && command -v import >/dev/null; then
    local id
    # Several apps may share the display with no window manager: bring this one to the top.
    "$GPUI_BIN" --x11-raise "$title" 2>/dev/null || true
    sleep 0.3
    id=$(xwininfo -root -tree 2>/dev/null | awk -v t="\"$title\":" 'index($0, t) { print $1; exit }')
    import -window "${id:-root}" "$png" 2>/dev/null || true
  elif [ -n "${WAYLAND_DISPLAY:-}" ] && command -v grim >/dev/null; then
    grim "$png" 2>/dev/null || true
  fi
}

# While the app ($1, pid $2, window title $3) runs: take each screenshot it asks for.
watch_shots() {
  local app=$1 pid=$2 title=$3 r name
  while kill -0 "$pid" 2>/dev/null; do
    for r in "$OUT/shots/$app"-*.ready; do
      [ -e "$r" ] || continue
      name=$(basename "$r" .ready)
      [ -n "${NO_SHOTS:-}" ] || capture "$title" "$OUT/shots/$name.png"
      mv "$r" "$OUT/shots/$name.taken"
    done
    sleep 0.05
  done
}

run() {
  local app=$1 n=$2 bin title
  if [ "$app" = gpui ]; then
    bin=$GPUI_BIN; title="Open Bible Software (GPUI text-paint prototype)"
  else
    bin=$EGUI_BIN; title="Open Bible Software"
  fi
  echo "== $app run $n"
  rm -f "$OUT/shots/$app"-*.taken
  env XDG_DATA_HOME="$SCRATCH/data" XDG_CONFIG_HOME="$SCRATCH/config" OBS_LIBRARY="$LIB" \
    OBS_PROTO_BENCH="$SCENARIO" OBS_PROTO_BENCH_OUT="$OUT/$app-$n.json" \
    OBS_PROTO_SHOT_DIR="$OUT/shots" \
    timeout 900 "$bin" > "$OUT/$app-$n.log" 2>&1 &
  local pid=$!
  watch_shots "$app" "$pid" "$title"
  wait "$pid" || echo "   $app exited with $? (see $OUT/$app-$n.log)"
  [ -f "$OUT/$app-$n.json" ] && echo "   wrote $OUT/$app-$n.json"
}

for n in $(seq 1 "$RUNS"); do
  run gpui "$n"
  sleep 2
  run egui "$n"
  sleep 2
done
rm -f "$OUT/shots/"*.taken

python3 "$HERE/compare.py" "$OUT" | tee "$OUT/summary.md"
echo
echo "Results in $OUT"
